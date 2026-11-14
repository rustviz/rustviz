#!/bin/bash
# runcov.sh : run rustviz html over every extracted program; one result line per file
export PATH=$RUSTVIZ_BIN:$PATH
cd $WORK/cov
mkdir -p html err
for f in aq/*.rs ls/*.rs; do
  id=$(basename $f .rs)
  rustviz html $f -o html/$id.html > /dev/null 2> err/$id.txt
  echo "$id $?"
done > results.txt
echo COV-DONE
