## Prereqs

### Hardware
Mostly this is storage heavy, though CPU cores help with parallel preprocessing, and you probably don't want to attempt 
this with less than 8GB ram, though I haven't tested this with anything less than 32GB. Having >1 Gbps internet
bandwidth will also help to speed up the large file downloads.

The raw lidar data is huge, you'll need ~1.9TB of free storage, ideally on an SSD, to follow along with these 
preprocessing steps. However, the final artifacts needed by the backend are smaller: around 95 GB, so they can be 
copied to a more flexible location for serving via HTTP to backend workers.

The workflow I use is to spin up a c8a.4xlarge AWS EC2 instance, with an attached gp3 SDD-backed volume,
configured for 1 GiB/s of disk throughput and 10k IOPS. This is expensive, so I keep it just long enough to run 
preprocessing before tearing down both the SDD and the instance. I copy the artifacts it produces to AWS S3, where they 
can be served to workers running the service backend. However, nothing about this project is tightly coupled to AWS, 
any Linux box with sufficient storage will work for preprocessing and any HTTP file server can act as the file provider
backend.

### Software
#### MacOS
```
brew install ncftp aria2 wget unzip node gdal pdal
```

```
npm install -g osmtogeojson
```

You'll also need docker. Install docker desktop from https://www.docker.com/products/docker-desktop/ if needed

#### Debian
```
sudo apt update
sudo apt install ncftp aria2 wget unzip nodejs npm gdal-bin pdal
```

```
npm install -g osmtogeojson
```

You'll also need docker:
```
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER && newgrp docker
```

## Build Preprocessor
```
cd preprocessing-rs/ && cargo build --release
```

## Download Data & Preprocess

```
cd preprocessing-rs/ && ./download.sh
```
(this can run for 1-3 hours depending on your internet connection)

```
cd preprocessing-rs/ && ./preprocess.sh
```
(this can run for 6-8 hours, depending on your CPU)

## Upload Preprocessing artifacts to File Server

### Option A: Use an HTTP server hosted on the preprocessing machine

To use the current machine as the file server, simply move the artifacts into a directory that is configured to act
as a static file HTTP server:
```
mv data/preprocessed-lidar-tiles/ <path-to-http-root>/preprocessed-lidar-tiles/
mv data/orthos/ <path-to-http-root>/ortho-photos/
mv data/obstructions/ <path-to-http-root>/simulated-obstructions/
mv data/footprint-wkt/ <path-to-http-root>/building-footprints/
```

### Option B: Use a remote self-hosted HTTP server as the file server

This is similar to option A, but separates the preprocessing machine from the file server. We provide a script to help
with the remote copying of the artifacts, use it like this:
```
cd preprocessing-rs/ && ./upload-scp.sh <file-server-hostname>:<path-on-file-server>
```
(this can run for several hours, depending on your connection speed to the file server)


### Option C: Use AWS S3 as the file server

If you don't want to use your own HTTP server, a good managed option for hosting static files is AWS S3. We provide a
script to help upload the files to your bucket:
```
cd preprocessing-rs/ && ./upload-s3.sh <s3-bucket-name> <s3-key-prefix>
```
(this can run for several hours, depending on your connection speed to AWS S3)

## Cleanup
You likely want to clean up the `data/` directory to free up space, since the intermediate artifacts are no longer
needed:
```
rm -rf data/
```

## Hosted Preprocessing
Another way to accomplish all of the above steps is to use the included `Dockerfile.preporcessing` 
container image to run on a managed container hosting solution, such as AWS Fargate. This section is purely
duplicative of everything outlined above, *there is no need to use AWS for this project to work*. However,
if you have an AWS account and want to use it complete the preprocessing steps, it can be more economical
and convenient than running an EC2 instance manually.

### Prerequisites
These steps assume you already have an S3 bucket to place the generated artifacts in. If not, create one now:
```
aws s3 mb s3://<insert bucket name here> --region <your aws region>
```
(note that bucket names must be unique globally across all AWS customers, you probably should include your account ID 
in the name to deduplicate)


Set some references we'll use later:
```
OUTPUT_S3_BUCKET=<bucket name from above or prexisting>
OUTPUT_S3_PREFIX=$(date +"%Y-%m-%d")
```

### Docker Image Upload
First, build the preprocessing docker image
```
docker build -f Dockerfile.preprocessing . -t loscope-preprocessing
```

Then create an ECR repository and upload the image we just built to it (replace with your AWS account ID)
```
AWS_ACCOUNT_ID=<your account id>
AWS_REGION=<your region>
aws ecr create-repository --repository-name loscope-preprocessing
aws ecr get-login-password --region ${AWS_REGION} | docker login --username AWS --password-stdin "${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com"
docker tag loscope-preprocessing:latest "${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/loscope-preprocessing:latest"
docker push "${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/loscope-preprocessing:latest"
```

Next we need to set up some supporting resources to enable the Fargate task to run:
```
aws ecs create-cluster --cluster-name loscope-preprocessing 
aws iam create-role --role-name ECS-role-Preprocessing --assume-role-policy-document "{\"Version\":\"2012-10-17\",\"Statement\":[{\"Effect\":\"Allow\",\"Principal\":{\"Service\":\"ecs-tasks.amazonaws.com\"},\"Action\":\"sts:AssumeRole\",\"Condition\":{\"StringEquals\":{\"aws:SourceAccount\":\"${AWS_ACCOUNT_ID}\"},\"ArnLike\":{\"aws:SourceArn\":\"arn:aws:ecs:${AWS_REGION}:${AWS_ACCOUNT_ID}:*\"}}}]}"
aws iam attach-role-policy --role-name ECS-role-Preprocessing --policy-arn arn:aws:iam::aws:policy/AmazonS3FullAccess
aws iam attach-role-policy --role-name ECS-role-Preprocessing --policy-arn arn:aws:iam::aws:policy/service-role/AmazonEC2RoleforSSM
aws iam create-role --role-name ECS-role-Preprocessing-executionrole --assume-role-policy-document '{"Version":"2008-10-17","Statement":[{"Sid":"","Effect":"Allow","Principal":{"Service":"ecs-tasks.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
aws iam attach-role-policy --role-name ECS-role-Preprocessing-executionrole --policy-arn arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy
aws iam create-role --role-name ECSServiceRoleForVolumes --assume-role-policy-document '{"Version":"2012-10-17","Statement":[{"Sid":"","Effect":"Allow","Principal":{"Service":"ecs.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
aws iam attach-role-policy --role-name ECSServiceRoleForVolumes --policy-arn arn:aws:iam::aws:policy/service-role/AmazonECSInfrastructureRolePolicyForVolumes
```

Next, draft an ECS task definition which bundles the docker image with some configuration information
so it is ready to run. You may want to change `ARM64` to `X86_64` if you built the docker image on x86 system.
```
cat > /tmp/task-definition.json <<EOF
{
    "family": "loscope-preprocessing",
    "containerDefinitions": [
        {
            "name": "loscope-preprocessing",
            "image": "${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/loscope-preprocessing:latest",
            "cpu": 0,
            "portMappings": [],
            "essential": true,
            "environment": [],
            "environmentFiles": [],
            "mountPoints": [
                {
                    "sourceVolume": "working-storage",
                    "containerPath": "/data",
                    "readOnly": false
                }
            ],
            "volumesFrom": [],
            "ulimits": [],
            "logConfiguration": {
                "logDriver": "awslogs",
                "options": {
                    "awslogs-group": "/ecs/loscope-preprocessing",
                    "awslogs-create-group": "true",
                    "awslogs-region": "us-east-1",
                    "awslogs-stream-prefix": "ecs"
                },
                "secretOptions": []
            },
            "systemControls": []
        }
    ],
    "taskRoleArn": "arn:aws:iam::${AWS_ACCOUNT_ID}:role/ECS-role-Preprocessing",
    "executionRoleArn": "arn:aws:iam::${AWS_ACCOUNT_ID}:role/ECS-role-Preprocessing-executionrole",
    "networkMode": "awsvpc",
    "volumes": [
        {
            "name": "working-storage",
            "configuredAtLaunch": true
        }
    ],
    "placementConstraints": [],
    "requiresCompatibilities": [
        "FARGATE"
    ],
    "cpu": "16384",
    "memory": "32768",
    "runtimePlatform": {
        "cpuArchitecture": "ARM64",
        "operatingSystemFamily": "LINUX"
    },
    "enableFaultInjection": false
}
EOF
```

Upload the draft config with:
```
aws ecs register-task-definition --family loscope-preprocessing --cli-input-json file:///tmp/task-definition.json
```

### Run Fargate Task

To start the preprocessing run, first fetch the subnet ID of the default VPC:
```
DEFAULT_VPC_ID=$(aws ec2 describe-vpcs --filters "Name=isDefault,Values=true" --query 'Vpcs[0].VpcId' --output text)
SUBNET_ID=$(aws ec2 describe-subnets --filters "Name=vpc-id,Values=${DEFAULT_VPC_ID}" --query 'Subnets[0].SubnetId' --output text)
SG_ID=$(aws ec2 describe-security-groups --filters "Name=vpc-id,Values=${DEFAULT_VPC_ID}" "Name=group-name,Values=default" --query 'SecurityGroups[0].GroupId' --output text)
```

Write the launch parameters to a file like:
```
cat > /tmp/run-task.json <<EOF
{
    "cluster": "loscope-preprocessing",
    "taskDefinition": "loscope-preprocessing",
    "launchType": "FARGATE",
    "platformVersion": "LATEST",
    "enableExecuteCommand": true,
    "networkConfiguration": {
        "awsvpcConfiguration": {
            "subnets": ["${SUBNET_ID}"],
            "securityGroups": ["${SG_ID}"],
            "assignPublicIp": "ENABLED"
        }
    },
    "overrides": {
        "containerOverrides": [
            {
                "name": "loscope-preprocessing",
                "environment": [
                    {"name": "OUTPUT_S3_BUCKET", "value": "${OUTPUT_S3_BUCKET}"},
                    {"name": "OUTPUT_S3_PREFIX", "value": "${OUTPUT_S3_PREFIX}"}
                ]
            }
        ]
    },
    "volumeConfigurations": [
        {
            "name": "working-storage",
            "managedEBSVolume": {
                "roleArn": "arn:aws:iam::${AWS_ACCOUNT_ID}:role/ECSServiceRoleForVolumes",
                "volumeType": "gp3",
                "sizeInGiB": 2000,
                "iops": 5000,
                "throughput": 1000,
                "filesystemType": "xfs",
                "encrypted": false,
                "terminationPolicy": {"deleteOnTermination": true}
            }
        }
    ]
}
EOF
```

And then launch the task with:
```
aws ecs run-task --cli-input-json file:///tmp/run-task.json --region ${AWS_REGION}
```

This should run for ~10 hours, and when it's completed, the specified S3 bucket should be populated with generated
artifacts. You can follow along with the progress by viewing the `/ecs/loscope-preprocessing` log group in the
CloudWatch console.

### Cleanup
There is an ongoing cost associated with storing the generated artifacts in S3 (approximately $2.30 / month plus 
$0.40 per million HTTP GET requests). We assume you don't want to clean this up since you just spent a bunch of effort
to generate it, but if needed, it can be cleaned up by emptying the bucket and then deleting it.

The use of Fargate with an ephemeral EBS volume mostly avoids ongoing costs for the computation parts, but you may want 
to clean up the ECR repository since there is a small ($0.20 / month) fee to keep a copy of the preprocessing docker 
image stored there.

To delete the ECR repo including all of its contents, just run:
```
aws ecr delete-repository --repository-name loscope-preprocessing --force
```

There are no ongoing costs associated with the following resources, but they may clutter your account, so you
may also want to do:
```
aws ecs list-task-definitions --family-prefix loscope-preprocessing \
    --query 'taskDefinitionArns[]' --output json | jq -r '.[]' | while read ARN; do
    aws ecs deregister-task-definition --task-definition "$ARN" > /dev/null
    aws ecs delete-task-definitions --task-definitions "$ARN" > /dev/null
    echo "Deleted $ARN"
done
aws ecs delete-cluster --cluster loscope-preprocessing
aws iam detach-role-policy --role-name ECS-role-Preprocessing --policy-arn arn:aws:iam::aws:policy/service-role/AmazonEC2RoleforSSM
aws iam detach-role-policy --role-name ECS-role-Preprocessing --policy-arn arn:aws:iam::aws:policy/AmazonS3FullAccess
aws iam detach-role-policy --role-name ECS-role-Preprocessing-executionrole --policy-arn arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy
aws iam detach-role-policy --role-name ECSServiceRoleForVolumes --policy-arn arn:aws:iam::aws:policy/service-role/AmazonECSInfrastructureRolePolicyForVolumes
aws iam delete-role --role-name ECS-role-Preprocessing
aws iam delete-role --role-name ECS-role-Preprocessing-executionrole
aws iam delete-role --role-name ECSServiceRoleForVolumes
```