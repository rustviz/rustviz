#!/bin/bash
ignore_list=(
    "extra_credit/"
)

for d in */ ; do
    if [[ ! " ${ignore_list[*]} " =~ " ${d} " ]]; then
        cd ..
        cargo run $d >/dev/null 2>&1
        cd ./examples
        input_fname="${d}input/annotated_source.rs"
        std_fname="${d}annotated_source.rs"
        echo "Compare $input_fname and $std_fname ..."
        DIFF=$(diff $input_fname $std_fname)
        if [ "$DIFF" != "" ] 
        then
            diff $input_fname $std_fname
        else
            echo "SUCCESS!"
        fi
    fi
done