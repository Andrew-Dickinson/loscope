## Prereqs

### Hardware
Mostly this is storage heavy, though CPU cores help with parallel preprocessing, and more RAM means
better disk caching.

You'll need ~700GB of free storage, ideally on an SSD, to follow along with the preprocessing steps.

### Software
#### MacOS
```
brew install ncftp aria2 wget unzip
```

You'll also need docker. Install docker desktop from https://www.docker.com/products/docker-desktop/ if needed

#### Debian
```
sudo apt install ncftp aria2 wget unzip
```

You'll also need docker:
```
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER && newgrp docker
```

## Build Preprocessor
```
cd preprocessing-rs/ && cargo build release
```

## Download Data & Preprocess

```
cd preprocessing-rs/ && ./download_and_preprocess.sh
```