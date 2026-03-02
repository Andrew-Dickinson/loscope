
To run preprocessing at scale, just use GNU parallel like so:
```
ls /mnt/nys_data/*.las | parallel -j16  --progress --joblog tmp/preprocess.log \
    python -m los_analyzer.preprocessing.preprocess {} /mnt/preprocessed
```