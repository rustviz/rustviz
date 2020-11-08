#!/bin/bash
red=$'\e[1;31m'
end=$'\e[0m'

cp ../mdbook_plugin/helpers.js helpers.js
cp ../mdbook_plugin/visualization.css visualization.css
mkdir -p "./theme"
cp ../mdbook_plugin/book.js theme/book.js

# Write the first line of SUMMARY.md. This clears anything that was there previously
printf "# Summary\n\n" > src/SUMMARY.md

printf "Generating visualizations for the following examples: \n"

# Uncomment the examples are being tested
declare -a targetExamples=(
    "hatra1"
    "hatra2"
    "string_from_print"
    "string_from_move_print"
    "func_take_ownership"
    "immutable_borrow"
    "multiple_immutable_borrow"
    "mutable_borrow"
    "nll_lexical_scope_different"
    "move_different_scope"
    "move_assignment"
    "move_func_return"
    "func_take_return_ownership"
    "immutable_borrow_method_call"
    #"error_use_after_move" # The "error_" examples are for visualizing Rust code with errors. This is not yet supported by RustViz.
    #"error_reassign_immutably_borrowed"
    #"error_reassign_mutably_borrowed"
    #"error_borrow_mutably_borrowed"
)

# Loop through the specified examples
for target in ${targetExamples[@]}; do
    printf "building %s..." $target
    
    # Check if required files are there
    if [[ -f  "../svg_generator/examples/$target/input/annotated_source.rs" && \
        -f "../svg_generator/examples/$target/main.rs" && -f "../svg_generator/examples/$target/source.rs" ]]
    then
        cd ../svg_generator
        # Run svg generation for example
        cargo run --example $target >/dev/null 2>&1

        # If if the svg generation exited with an error or the required SVGs weren't created, report failure and continue
        if [[ $? -ne 0 || !(-f "./examples/$target/vis_code.svg") || !(-f "./examples/$target/vis_timeline.svg") ]]; then
            printf "${red}FAILED${end} on SVG generation.\n"
            cd ../test_examples
            continue
        fi
        cd ../test_examples

        # Copy files to mdbook directory
        mkdir -p "./src/assets/$target"
        cp "../svg_generator/examples/$target/source.rs" "./src/assets/$target/source.rs"
        cp "../svg_generator/examples/$target/vis_code.svg" "./src/assets/$target/vis_code.svg"
        cp "../svg_generator/examples/$target/vis_timeline.svg" "./src/assets/$target/vis_timeline.svg"
        
        # Add append corresponding line to SUMMARY.md
        echo "- [$target](./$target.md)" >> src/SUMMARY.md
        printf "\n"
    else
        # Report failure if required files aren't there
        printf "${red}FAILED${end}. The required files are not in the examples dir.\n"
    fi
done

# Build mdbook
mdbook build

# Run HTTP server on docs directory
cd book
python3 -m http.server