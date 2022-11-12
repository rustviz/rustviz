#!/bin/bash
red=$'\e[1;31m'
end=$'\e[0m'

# copy book.js to theme/
mkdir -p "./theme"
cp mdbook_plugin/book.js theme/book.js

if ! [[ -d "src" ]]; then
    mkdir src
fi

# clear assets and md files to mdbook directory
rm -f src/*md

if [[ -d "src/assets" ]]; then
    rm -r src/assets
fi

# Write the first line of SUMMARY.md. This clears anything that was there previously
printf "# Summary\n\n" > src/SUMMARY.md

printf "Generating visualizations for the following examples: \n"

# Uncomment the examples are being tested
declare -a targetExamples=(
    "copy"
    "func_take_ownership"
    "func_take_return_ownership"
    "function"
    "hatra1"
    "hatra1_test"
    "hatra2"
    "immutable_borrow"
    "immutable_borrow_lifetime"
    "immutable_borrow_method_call"
    "immutable_variable"
    "move_assignment"
    "move_different_scope"
    "move_func_return"
    "multiple_immutable_borrow"
    "mutable_borrow"
    "mutable_borrow_method_call"
    "mutable_variables"
    "nll_lexical_scope_different"
    "printing"
    "string_from_move_print"
    "string_from_print"
    "struct_lifetime"
    "struct_rect"
    "struct_rect2"
    "struct_string"
    "extra_credit"
)

EX="../src/examples"
# Loop through the specified examples
for target in ${targetExamples[@]}; do
    printf "building %s..." $target
    
    # Check if required files are there
    if [[ -f  "$EX/$target/input/annotated_source.rs" && -f "$EX/$target/source.rs" ]]
    then
        # Check if file headers exist
        if ! [[ -f "$EX/$target/main.rs" ]]
        then
            printf "\ngenerating header for %s..." $target
            cd ../RustvizParse
            cargo run "$EX/$target/source.rs" >/dev/null 2>&1
        fi

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

        # Write into .md files
        printf "### %s\n\n" "$target" >> src/$target.md
        printf "\`\`\`rust\n" >> src/$target.md
        printf "{{#rustdoc_include assets/%s/source.rs}}\n" "$target" >> src/$target.md
        printf "\`\`\`\n" >> src/$target.md
        printf '<div class="flex-container vis_block" style="position:relative; margin-left:-75px; margin-right:-75px; display: flex;">\n' >> src/$target.md
        printf '\t<object type="image/svg+xml" class="%s code_panel" data="assets/%s/vis_code.svg"></object>\n' "$target" "$target">> src/$target.md
        printf '\t<object type="image/svg+xml" class="%s tl_panel" data="assets/%s/vis_timeline.svg" style="width: auto;" onmouseenter="helpers('"'"'%s'"'"')"></object>\n' "$target" "$target" "$target">> src/$target.md
        printf "</div>" >> src/$target.md
    else
        # Not Necessary (file double check)
        printf "${red}FAILED${end}. The required files are not in the examples dir.\n"
    fi
done

# Build mdbook
mdbook build

# Run HTTP server on docs directory
mdbook serve -p 8000
