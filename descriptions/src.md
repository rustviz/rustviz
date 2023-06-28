## src -- parser.rs

**`parse_vars_to_map`**: This function takes a file path as input and returns a tuple containing the lines of the file, the number of lines, and a HashMap that maps variable names to ResourceAccessPoint values. It reads the file, checks for a specific template in the first line, parses variable definitions into a string, splits the string into individual variables, and then converts them into key-value pairs in the HashMap.

**`vec_to_map`**: This function takes a vector of variable strings and converts them into a HashMap of variable names mapped to **`ResourceAccessPoint`** values. It iterates over the parsed strings, extracts the type, mutability, and name fields, and based on the type, creates the corresponding **`ResourceAccessPoint`** enum variant.

**`extract_events`**: This function takes the lines of a file and the line number of the main code block as input and returns a vector of tuples representing events extracted from the file. It iterates over the lines, detects block comments that contain events (denoted by **`!{}`**), and extracts the events from those comments. It handles cases where block comments span multiple lines and appends the extracted events to the result vector.

**`add_events`**: This function takes a mutable reference to a **`VisualizationData`** struct, a HashMap of variable definitions, and a vector of events, and adds those events to the visualization data. It iterates over the events, parses the event string into its components, and based on the event type, adds the corresponding **`ExternalEvent`** variant to the visualization data.

**`get_resource`**: This function takes a HashMap of variable definitions and a variable name as input and returns the corresponding ResourceAccessPoint value if it exists in the HashMap. If the variable name is `"None"`, it returns None. If the variable is not found in the HashMap, it prints an error message and exits the program.

**`get_name_field`**: This function takes a vector of fields and returns the name field as a string. It is used to extract the name field from the parsed variable fields.

**`get_mut_qualifier`**: This function takes a vector of fields and returns a boolean indicating whether the mutability qualifier is present. If the qualifier is not recognized or there are incorrect qualifiers/fields, it prints an error message and exits the program.

**`print_var_usage_error`**: This function prints an error message to stderr indicating incorrect variable formatting.

**`event_usage_err`**: This function returns a formatted event usage error message as a string.

**`delimitation_err`**: This function prints an error message indicating an unterminated delimitation in the file and exits the program.

## src -- main.rs

The **`main`** functions runs in 5 stages:

1. **Argument verification**: It checks whether the user provided exactly one argument (which is expected to be a filename) to the script when it was called from the command line. If the user didn't provide exactly one argument, the script prints an error message and exits.

2. **Checking directory and file**: The script checks if there's a directory in the examples folder with the name provided by the user. It also checks if a file named main.rs exists in this directory.

3. **Parsing the Rust file**: It calls parse::parse_vars_to_map(filename) to parse the Rust file and get its contents, line numbers, and a mapping of variables (var_map). The parse::extract_events(contents, line_num) function extracts the events from the contents of the Rust file.

4. **Building VisualizationData**: It creates an instance of VisualizationData and populates it with events using the parse::add_events(&mut vd, var_map, events) function.

5. **SVG Generation**: It uses the svg_generation::render_svg() function to generate SVG images. This function takes the input file path, output file path, and the visualization data object as arguments. It saves the generated SVG images in the directory specified by the user.