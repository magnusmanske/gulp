#!/usr/bin/bash
\rm target/release/gulp
time jsub -N build -mem 2G -sync y -cwd cargo build --release
ls -l target/release/gulp
