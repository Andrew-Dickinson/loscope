How to use PDAL to filter out noisy Lidar points before conducting preprocessing:
```
docker run -v /Users/mesh/PycharmProjects/los-analyzer-4:/work pdal/pdal \
    pdal pipeline /work/tools/preprocessing/denoise_pipeline.json \
        --readers.las.filename=/work/data/nys_raw/7250.las \
        --writers.las.filename=/work/data/pdal-out/7250-denoise-k12m40.las
```