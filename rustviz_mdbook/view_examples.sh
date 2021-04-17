#!/bin/bash
red=$'\e[1;31m'
end=$'\e[0m'

# copy book.js to theme/
mkdir -p "./theme"
cp mdbook_plugin/book.js theme/book.js

# Write the first line of SUMMARY.md. This clears anything that was there previously
printf "# Summary\n\n" > src/SUMMARY.md

printf "Generating visualizations for the following examples: \n"

# Uncomment the examples are being tested
declare -a targetExamples=(
    # "copy"
    # "func_take_ownership"
    # "func_take_return_ownership"
    # "function"
    # "hatra1"
    # "hatra2"
    # "immutable_borrow"
    # "immutable_borrow_method_call"
    # "immutable_variable"
    # "move_assignment"
    # "move_different_scope"
    # "move_func_return"
    # "multiple_immutable_borrow"
    # "mutable_borrow"
    # "mutable_borrow_method_call"
    # "mutable_variables"
    # "nll_lexical_scope_different"
    # "printing"
    # "string_from_move_print"
    # "string_from_print"
    # "struct_lifetime"
    # "struct_rect"
    # "struct_rect2"
    # "struct_string"
    "extra_credit"
)

EX="../src/examples"
# Loop through the specified examples
for target in ${targetExamples[@]}; do
    printf "building %s..." $target
    
    # Check if required files are there
    if [[ -f  "$EX/$target/input/annotated_source.rs" && \
        -f "$EX/$target/main.rs" && -f "$EX/$target/source.rs" ]]
    then
        cd ../src # switch to appropriate folder
        # Run svg generation for example
        cargo run $target >/dev/null 2>&1

        # If if the svg generation exited with an error or the required SVGs weren't created, report failure and continue
        if [[ $? -ne 0 || !(-f "examples/$target/vis_code.svg") || !(-f "examples/$target/vis_timeline.svg") ]]; then
            printf "${red}FAILED${end} on SVG generation.\n"
            cd ../rustviz_mdbook
            continue
        fi
        cd ../rustviz_mdbook

        # Copy files to mdbook directory
        mkdir -p "./src/assets/$target"
        cp "$EX/$target/source.rs" "./src/assets/$target/source.rs"
        cp "$EX/$target/vis_code.svg" "./src/assets/$target/vis_code.svg"
        cp "$EX/$target/vis_timeline.svg" "./src/assets/$target/vis_timeline.svg"
        
        # Add append corresponding line to SUMMARY.md
        echo "- [$target](./$target.md)" >> src/SUMMARY.md
        echo "done"
    else
        # Report failure if required files aren't there
        printf "${red}FAILED${end}. The required files are not in the examples dir.\n"
    fi
done

# Build mdbook
mdbook build

# Run HTTP server on docs directory
cd book
python3 -m http.server 8000