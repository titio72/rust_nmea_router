#!/bin/sh
mysqldump --user=$1 --password=$2 nmea_router vessel_status environmental_data trips | gzip > $3

