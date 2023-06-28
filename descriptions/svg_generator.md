## src -- svg_generator -- svg_frontend

### code_panel.rs

**`render_code_panel`**: This function  is responsible for generating a representation of a code segment using SVG format. It accepts several parameters:
    - **`annotated_lines`**: A buffered reader that reads lines from a file.
    - **`lines`**: Another buffered reader that reads lines from a file.
    - **`max_x_space`**: A mutable reference to a 64-bit integer.
    - **`event_line_map`**: A binary tree map that maps line numbers to a vector of **`ExternalEvent`** objects.

This function works in 6 stages:
1. Initialize the **`Handlebars`** templating engine and register a template string **`line_template`** to render each line of code. It also disables HTML escaping to preserve the inputs as is.
2. Iterates over **`lines`** to figure out the maximum length of a line. This is used to set the **`max_x_space`** parameter, ensuring there's sufficient horizontal space to render the longest line of code.
3. Initializes some variables for rendering the SVG. **`x`** and **`y`** are the starting coordinates for the code panel, **`output`** is a string that will hold the SVG content, and **`line_of_code`** tracks the current line number.
4. Loops over **`annotated_lines`**. For each line of annotated code, it creates a mapping to be used with the Handlebars template, adds the line of code to the SVG output (**`output`**), and increments the y-coordinate for the next line.
5. Checks if there are any events associated with the current line of code in **`event_line_map`**. If there are multiple events, it adds extra lines for arrows.
6. Finally, it closes the SVG group (**`<g>`**) and returns the **`output`** string and the last line number as a tuple.

### svg_generations.rs

**`render_svg`**: The function render_svg generates SVG (Scalable Vector Graphics) visualizations of some code. Its primary purpose is to analyze a piece of Rust code, capture the lifetimes of variables and their relationships, and output a visual timeline that shows when each variable is created, borrowed, mutated, or dropped.

The function takes three parameters:
    - **`input_path`**: The path to the input file.
    - **`output_path`**: The path to the output file where the SVG visualizations will be saved.
    - **`visualization_data`**: A mutable reference to a **`VisualizationData`** object which encapsulates the data necessary for the visualization.

WorkFlow:
1. **Sort events:** It sorts the events in **`event_line_map`** by their "from" and "to" points. Events are sorted first by the "to" points and then by the "from" points.
2. **Update line numbers for external events:** The function calculates the final line number for each external event taking into account any extra lines added due to multiple events on the same line.
3. **Update event_line_map line numbers:** It then updates the line numbers in **`event_line_map`** to account for the extra lines.
4. **Read SVG and CSS templates:** It reads in template files for the SVG visualizations and CSS styles from the filesystem.
5. **Render code and timeline panels:** It then calls **`render_code_panel`** and **`render_timeline_panel`** functions to generate SVG strings for the code panel and timeline panel respectively.
6. **Prepare SVG data:** A **`SvgData`** struct is created with the necessary information such as the SVG and CSS strings, visualization name, timeline width, and height.
7. **Render final SVGs:** It uses the Handlebars templating library to render the final SVGs by substituting the actual data into the SVG templates.
8. **Write to output files:** Finally, it writes the generated SVG code and timeline visualizations to the specified output files.

### timeline_panels.rs
Generally, this file is primarily focused on creating a timeline visualization of various events.

- **`prepare_registry(&mut registry)`** function: The function sets up a **`Handlebars`** registry with various HTML templates. The **`assert!`** function ensures that the template registration is successful.

- **`render_timeline_panel(visualization_data : &VisualizationData) -> (String, i32)`** function: The main function for rendering a timeline panel. It calculates the layout, creates SVG elements (like timelines, labels, dots, arrows, etc.) and returns the final SVG string and its width. The SVG string can then be embedded into an HTML page to display the timeline.
    - **`render_timelines(&mut output, visualization_data, &resource_owners_layout, &registry)`**, **`render_labels_string(&mut output, &resource_owners_layout, &registry)`**, **`render_dots_string(&mut output, visualization_data, &resource_owners_layout, &registry)`**, **`render_ref_line(&mut output, visualization_data, &resource_owners_layout, &registry)`**, **`render_arrows_string_external_events_version(&mut output, visualization_data, &resource_owners_layout, &registry)`** and **`render_struct_box(&mut output, &structs_info, &registry)`** are functions responsible for rendering different parts of the timeline (timelines, labels, dots, lines, arrows, and struct boxes).
    - **`registry.render("timeline_panel_template", &timelinepanel).unwrap()`** and **`registry.render("timeline_panel_template", &member_timelinepanel).unwrap()`** commands take the Handlebars template registered with the name "timeline_panel_template" and provide it with the corresponding data to generate SVG markup.

- **`compute_column_layout`**: This function is responsible for organizing the resources (variables, struct members, etc.) into a layout, sorted based on their access pattern. Each resource gets a column, where its name, position (x_val), and other details are stored. The function also handles specific cases like mutable/immutable resources and different types of resource access points (e.g., Struct, Owner, MutRef, StaticRef). It's also looking for groups of resources that belong to the same structure (is_struct_group) and members of those structures (is_member), storing this information for later usage.

- **`render_labels_string`**: This function is generating a string representation of labels for the visual output. It iterates over each resource owner and generates a string to represent it, which includes its name, its hash, its x_val (which would probably correspond to its position in the visual layout), and its title (a stylized name with extra information like mutability). It also handles references, and structure members by appending corresponding labels to the respective owners.

- **`render_dots_string`**: This function seems to be creating a string representation of the events that each resource goes through, like acquiring a resource, moving a resource, or a resource going out of scope. This function is creating a "dot" for each of these events. The position (x and y coordinates) of these dots probably represents the resource (x) and the time/line number when the event occurred (y). The details of the event are added to the title, which can include messages like whether a resource was dropped when an owner went out of scope.

- **`determine_owner_line_styles`:**
    - The function takes two parameters: **`rap`**, which is a reference to a **`ResourceAccessPoint`** object, and **`state`**, which is a reference to a **`State`** object. The ampersand (&) before these parameters denotes that they are borrowed, i.e., the function doesn't own them and they will not be dropped when the function ends.
    - The **`match`** expression is used to select a pattern that matches the provided (state, rap.is_mut()) tuple. This behaves somewhat similarly to a switch statement in other languages, but is more powerful.
    - **`rap.is_mut()`** is a function call that checks if the resource access point is mutable, i.e., if its state can be changed. If it's mutable, it returns **`true`**, otherwise it returns **`false`**.
    - **`State::FullPrivilege`** and **`State::PartialPrivilege{..}`** are patterns that match the **`state`**. In Rust, **`::`** is used to denote a particular variant of an enum.
    - **`OwnerLine::Solid`**, **`OwnerLine::Hollow`**, and **`OwnerLine::Empty`** are possible return values of the function. They are variants of the **`OwnerLine`** enum.
    - The **`=>`** operator separates patterns from what should be executed if the pattern matches.

- **`create_owner_line_string`:** This function ****generates a string representation of the owner line based on a Resource Access Point (RAP), its State, a mutable reference to **`VerticalLineData`**, and a **`Handlebars`** registry for templating. The function returns the generated string.
    - **`determine_owner_line_styles(rap, state)`**: This is a function call to **`determine_owner_line_styles`** which you provided earlier. It determines the style of the line based on **`rap`** and **`state`**.
    - **`match (state, style)`**: This starts a **`match`** expression on a tuple made from **`state`** and **`style`**.
    - The different match arms handle the different combinations of **`state`** and **`style`**:
        - If the state is **`FullPrivilege`** or **`PartialPrivilege`** and the style is **`Solid`**, it updates the **`line_class`** and **`title`** fields of **`data`** and uses **`registry`** (a Handlebars template engine) to render a template named "vertical_line_template" with the updated **`data`**.
        - If the state is **`FullPrivilege`** or **`PartialPrivilege`** and the style is **`Hollow`**, it creates a mutable clone of **`data`**, updates some fields, and then uses the Handlebars registry to render a template named "hollow_line_template".
        - If the state is **`OutOfScope`**, it simply returns an empty string.
        - In any other case (including **`RevokedPrivilege`**, **`OutofScope`**, **`ResourceMoved`**), it also returns an empty string.

- **`create_reference_line_string`** takes four arguments - a reference to a **`ResourceAccessPoint`** object, a reference to a **`State`** object, a mutable reference to **`VerticalLineData`**, and a **`Handlebars`** registry. It returns a string that visually represents the line of reference, based on the resource access point and its state. The line style, title, and other properties are adjusted depending on the state and the mutability of the resource access point.

- **`render_timelines`** function generates visual timelines for Resource Access Points (RAPs) using vertical lines. This is a more complex function that involves iterating through multiple data structures and updating them. It takes four arguments:
    - **`output`**: a mutable reference to a **`BTreeMap`**. This map is updated during the function with the timeline data.
    - **`visualization_data`**: a reference to the data that is going to be visualized.
    - **`resource_owners_layout`**: a reference to a **`BTreeMap`** that seems to contain layout information about how each resource owner is positioned on the timeline.
    - **`registry`**: a **`Handlebars`** registry, used to render HTML templates for the visualization.
        
        This function goes through each timeline in **`visualization_data.timelines`** and retrieves the **`ResourceAccessPoint`** (RAP) and its associated states.
        
    
    For each state in a RAP's timeline, it creates **`VerticalLineData`** (unless the RAP is a function, in which case it skips the iteration). The **`VerticalLineData`** contains visual properties for a line in the visualization, such as its class, position (x1, y1, x2, y2), hash, and title.
    
    It then checks the type of RAP. If it's an **`Owner`** or **`Struct`**, it checks if it belongs to a struct group. If it does, it updates the **`output`** map with the rendered owner line string for that RAP and state. If the RAP is a member of a struct, the line string is added to the **`timelines`** field of the second element of the tuple in the map's value. If it's not a member, it's added to the first element.
    
    If the RAP is not part of a struct group, it simply adds the line string to the **`timelines`** field of the first element of the tuple in the map's value at key -1.
    
    If the RAP is a **`StaticRef`** or **`MutRef`**, it updates the **`output`** map at key -1 with the rendered reference line string for that RAP and state.
    
    So the core purpose of this function is to render visual representations of the states of different RAPs over time and organize them into an output map for further usage.
    
- **`render_ref_line`**: This function generates the visual representation of reference lines in the timeline. A reference line indicates whether a mutable reference can mutate its resource (if it has **`FullPrivilege`**).

It iterates over each timeline in **`visualization_data.timelines`** and for each **`ResourceAccessPoint`** (RAP) which isn't a **`Function`**, it retrieves its associated states. It then determines if the RAP is alive (i.e., it is either in a **`FullPrivilege`** or **`PartialPrivilege`** state) and updates the **`data`** accordingly.

For **`MutRef`** and **`StaticRef`**, it concludes their lines when their state is **`OutOfScope`** or **`ResourceMoved`** by updating the end points (**`x2`** and **`y2`**), calculating the **`v`** and **`dy`** values, and rendering the line templates (**`solid_ref_line_template`** or **`hollow_ref_line_template`**).

- **`render_struct_box`**: This function generates a visual box around structs in the timeline. It iterates over **`structs_info.structs`** and for each struct, it calculates the dimensions (**`x`**, **`w`**, **`h`**) and position (**`y`**) of the box. Then it renders the box using the **`box_template`**.

- **`get_y_axis_pos`**: This is a helper function which calculates the y-axis position for a given line number. It uses the constant **`LINE_SPACE`** to ensure equal spacing between lines. The line_number is assumed to start at 0, and the y-axis position is calculated such that line 0 is positioned at **`85 - LINE_SPACE`** pixels from the top, and each subsequent line is **`LINE_SPACE`** pixels below the previous line. This function ensures that all lines are evenly spaced vertically.

### utils.rs

- **`read_file`**: This function accepts a file path and opens the file in read mode. The file is then returned as a buffered reader wrapped in a **`Result`** for error handling. The function is generic over the parameter type for **`file_path`**, which can be any type that implements the **`AsRef<Path>`** trait.
- **`read_file_to_string`**: This function opens the file (like the **`read_file`** function), reads the entire content into a **`String`**, and returns it. If any error occurs during these operations, it will return an **`io::Result::Err`**.
- **`read_lines`**: This function opens a file and returns a **`Lines`** iterator from the standard library. This iterator will yield instances of **`io::Result<String>`** for each line in the file.
- **`create_and_write_to_file`**: This function creates a file at the specified path and writes the given string to it. If it encounters any error during these operations, it will **`panic`** and terminate the program. The function does not return any value.

In all these functions, the **`P: AsRef<Path>`** parameter type allows the caller to pass file paths in various formats (like **`String`**, **`str`**, **`Path`**, **`PathBuf`**, etc.). The **`as_ref()`** method is called on the file path to convert it into a **`&Path`** reference.