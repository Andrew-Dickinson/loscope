
To run preprocessing at scale, just use GNU parallel like so:
```
ls /mnt/nys_data/*.las | cut -d. -f1 | cut -d"/" -f4 | parallel -j4 --progress --joblog tmp/preprocess.log \
    docker run \
        -v /home/ubuntu/los-analyzer-4/tools/preprocessing/denoise_pipeline.json:/denoise_pipeline.json \
        -v /mnt:/mnt \
        pdal/pdal \
            pdal pipeline /denoise_pipeline.json \
                --readers.las.filename=/mnt/nys_data/{}.las \
                --writers.las.filename=/mnt/denoised/{}.las
```


```
ls /mnt/denoised/*.las | parallel -j16  --progress --joblog tmp/preprocess.log \
    python -m los_analyzer.preprocessing.preprocess {} /mnt/preprocessed
```