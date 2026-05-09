#!/usr/bin/env bash
# Download one year of hourly solar PV generation as CSV from PVGIS (EU JRC).
# Endpoint docs: https://joint-research-centre.ec.europa.eu/photovoltaic-geographical-information-system-pvgis/getting-started-pvgis/api-non-interactive-service_en
#
# Usage:
#   ./download_solar.sh                       # defaults: Madrid, 2020, 5kW, tilt 35, south
#   ./download_solar.sh <lat> <lon> <year>    # override site and year
#   ./download_solar.sh <lat> <lon> <year> <out.csv>
#
# Output: 8760 hourly rows of P (power, W), G(i), H_sun, T2m, WS10m for the chosen year.

set -euo pipefail

LAT=${1:-40.4168}
LON=${2:--3.7038}
YEAR=${3:-2020}
OUT=${4:-"solar_${LAT}_${LON}_${YEAR}.csv"}

PEAKPOWER=5      # kWp
LOSS=14          # % system losses
ANGLE=35         # tilt in degrees
ASPECT=0         # 0 = south
MOUNTING=free    # free-standing

URL="https://re.jrc.ec.europa.eu/api/seriescalc"
URL+="?lat=${LAT}&lon=${LON}"
URL+="&startyear=${YEAR}&endyear=${YEAR}"
URL+="&pvcalculation=1"
URL+="&peakpower=${PEAKPOWER}"
URL+="&loss=${LOSS}"
URL+="&mountingplace=${MOUNTING}"
URL+="&angle=${ANGLE}&aspect=${ASPECT}"
URL+="&outputformat=csv"

echo "Fetching $URL"
curl -fSL --retry 4 --retry-delay 2 -o "$OUT" "$URL"
echo "Wrote $OUT ($(wc -l < "$OUT") lines)"
