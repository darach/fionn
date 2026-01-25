FROM gcr.io/oss-fuzz-base/base-builder-rust@sha256:71543bf6c750489e46e006de154cedd882c0543c4100ba3161d02b3c974720cc

COPY . $SRC/fionn
WORKDIR $SRC/fionn

COPY .clusterfuzzlite/build.sh $SRC/
