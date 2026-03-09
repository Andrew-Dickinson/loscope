#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-data/city_data}"
mkdir -p "$OUT_DIR"

download() {
    local name="$1"
    local id="$2"
    local out="$OUT_DIR/${name}_${id}.csv"
    echo "Downloading $name ($id)..."
    wget -q --show-progress \
        "https://data.cityofnewyork.us/resource/${id}.csv?\$limit=99999999999" \
        -O "$out"
    echo "Saved to $out"
}

download "building-footprints"               "5zhs-2jue"
download "DOB-NOW-Certificate-of-Occupancy"  "pkdm-hqz6"
download "Digital-Tax-Map-Condominiums"      "p8u6-a6it"
download "DOB-Job-Application-Filings"       "ic3t-wcy2"
download "DOB-NOW-Build-Job-Application-Filings" "w9ak-ipjd"
download "DOB-Permit-Issuance"               "ipu4-2q9a"
download "DOB-NOW-Build-Approved-Permits"    "rbx6-tga4"
download "TAX_LOT_POLYGON"                   "i38t-6if2"

echo "Done."