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
brew install ncftp aria2 wget unzip node gdal
```

```
npm install -g osmtogeojson
```

You'll also need docker. Install docker desktop from https://www.docker.com/products/docker-desktop/ if needed

#### Debian
```
sudo apt update
sudo apt install ncftp aria2 wget unzip nodejs npm gdal-bin
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