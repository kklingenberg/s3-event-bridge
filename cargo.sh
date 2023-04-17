#!/usr/bin/env bash

set -e

CWD=$(pwd)

if ! test -d "${CWD}/build-libs/libjq"
then
  mkdir -p "${CWD}/build-libs"
  wget https://github.com/flant/libjq-go/releases/download/jq-b6be13d5-0/libjq-glibc-amd64.tgz \
       -O "${CWD}/build-libs/libjq-glibc-amd64.tgz"
  tar -C "${CWD}/build-libs" -xzf "${CWD}/build-libs/libjq-glibc-amd64.tgz"
fi

exec env \
     JQ_LIB_STATIC=1 \
     JQ_LIB_DIR="${CWD}/build-libs/libjq/lib" \
     cargo "$@"
