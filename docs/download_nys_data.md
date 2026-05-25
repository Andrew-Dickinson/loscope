
The 2021 NYC Lidar data can be found here:
```
ftp://ftp.gis.ny.gov/elevation/LIDAR/NYC_2021/
```

This page lets you explore the grid divisions and identify specific files (turn on the Lidar layer):
https://orthos.dhses.ny.gov/#

The total dataset is ~700GB, which can take a while to download. 

A simple CLI command to download the whole dataset is:
```commandline
wget -r ftp://ftp.gis.ny.gov/elevation/LIDAR/NYC_2021/
```

Probably you want to run this somewhere with very high bandwidth (i.e. a server in a datacenter). To make sure you're
saturating the network bandwidth available to the server, it's nice to download files in parallel.

Here's an example of one way to do that:
```
ncftpls ftp://ftp.gis.ny.gov/elevation/LIDAR/NYC_2021/ > /tmp/remote_files.txt
sed -i -e 's/^/ftp:\/\/ftp.gis.ny.gov\/elevation\/LIDAR\/NYC_2021\//' /tmp/remote_files.txt
aria2c -x 1 -j 3 -i /tmp/remote_files.txt
```

Use iftop to monitor the bandwidth to make sure the download is proceeding efficiently:
```
sudo iftop
```


### Orthos

Orthos are available at `ftp://ftp.gis.ny.gov//ortho/nysdop10/new_york_city/spcs/`, use a similar command
to download them all:

```commandline
wget -r ftp://ftp.gis.ny.gov//ortho/nysdop10/new_york_city/spcs/zips
```