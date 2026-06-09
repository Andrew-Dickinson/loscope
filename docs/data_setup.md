## Prereqs

### Hardware
Mostly this is storage heavy, though CPU cores help with parallel preprocessing, and more RAM means
better disk caching.

You'll need ~???GB of free storage, ideally on an SSD, to follow along with the preprocessing steps. 
However, the final artifacts are smaller: around ?? GB.

You can clean up the intermediate preprocessing artifacts when it's finished to free up space.

### Software
#### MacOS
```
brew install ncftp aria2 wget unzip node
```

```
npm install -g osmtogeojson
```

You'll also need docker. Install docker desktop from https://www.docker.com/products/docker-desktop/ if needed

#### Debian
```
sudo apt update
sudo apt install ncftp aria2 wget unzip nodejs npm
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
cd preprocessing-rs/ && cargo build release
```

## Download Data & Preprocess

```
cd preprocessing-rs/ && ./download_and_preprocess.sh
```