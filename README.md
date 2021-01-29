# RustViz
[![Build Status](https://travis-ci.org/joemccann/dillinger.svg?branch=master)](https://travis-ci.org/joemccann/dillinger)

*RustViz* is a tool written in Rust that generates visualizations from simple Rust programs to assist potential users and students in better understanding the Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) mechanism.

## What does it look like?

*RustViz* generates *.svg* files of graphical indicators that integrate with [mdbook](https://github.com/rust-lang/mdBook) to generate visualization over user-defined rust code examples. Here's a sample view of what the visualization looks like:

![alt tag](https://github.com/rustviz/rustviz/blob/master/example.png)

## Usage (example)
*RustViz* is capable of visualizing simple rust codes (refer to the restriction section) via user definition. In this section we'll showcase how to generate some default visulization example that has been provided by us.

*RustViz* requires [Rust](https://www.rust-lang.org/), Cargo and [mdbook](https://github.com/rust-lang/mdBook) to be installed. Once you have installed all the above prerequisites, direct into the */test_example* folder and run *test_examples.sh*
```
./test_examples.sh
```
You may have the a output that's similar to this:
```
Generating visualizations for the following examples: 
building hatra1...
building hatra2...
building string_from_print...
building string_from_move_print...
building func_take_ownership...
building immutable_borrow...
building multiple_immutable_borrow...
building mutable_borrow...
building nll_lexical_scope_different...
building move_different_scope...
building move_assignment...
building move_func_return...
building func_take_return_ownership...
building immutable_borrow_method_call...
building mutable_borrow_method_call...
building immutable_variable...
building mutable_variables...
building copy...
building function...
building printing...
2021-01-19 12:36:13 [INFO] (mdbook::book): Book building has started
2021-01-19 12:36:13 [INFO] (mdbook::book): Running the html backend
Serving HTTP on :: port 8000 (http://[::]:8000/) ...
```
If you observed this output, then you have successfully generated the rust visulization examples! Now open your brower and browse into *http://localhost:8000/*. You should be able to view all the examples by selecting each from the list bar on the left. To enable visulization, toggle the swtich that is included in the code section.

Great! Now you've know how to generate and view the visualization that you could create by using *RustViz*, Now let's create one of your own!

## Usage (advanced)
In this section, we'll take a look into how to create example by using our example [string_from_move_print](svg_generator/examples/string_from_move_print). let's first take a look at the file structure you need for the example to run:
```
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
└── source.rs
```
let's first take a look at the [source.rs](), which is simply the rust source code that we are generating visulization from:
```
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y)
}
```
In this example, the string `hello`'s resource is first moved from `String::from()` to `x`, then `x`'s resource is moved to `y`. Lastly, we print the value by taking `y` as an input to `println!()` but the resource has not been moved. 

Next, let's focus on we need to do in [main.rs](). In this visuliation tool, **we define all possible owners, references or input of any memory resource as a** [Resource Access Point](). In this case, we have the function `String::from()` and two variables `x` and `y` as Resource Access Points. Correspondingly in our implementation, the [Resource Access Point]() is defined as an enum that hold the possible types of Resource Access Points, namely `ResourceAccessPoint::Owner` and `ResourceAccessPoint::Function` in this case. We want to create instance that represent these functions and variables in our main program:
```
// Variables
    let x = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("x"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move
    });
    let y = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("y"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move
    });
// Functions
    let from_func = ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    });
```
Next we decalre an instance of the [VisualizationData]() struct as a container that holds all the information of [ExternalEvent]() that we will talk about up next:
```
let mut vd = VisualizationData {
    timelines: BTreeMap::new(),
    external_events: Vec::new(),
    preprocess_external_events: Vec::new(),
    event_line_map: BTreeMap::new()
};
```
The [ExternalEvent]() **is an enum that hold all the movement, borrowing and dropping of a resource.** In our case, we have four of such event: 
1. Resource was moved from `String::from()` to `x`
2. Resource was moved from `y` to `x`
3. Resource of `x` is dropped
4. Resource of `y` is dropped

We then add these events information to the [VisualizationData]() instance we declared before by using the `append_external_event()` function:
```
// Resource was moved from `String::from()` to `x`
    vd.append_external_event(ExternalEvent::Move{from: Some(from_func.clone()),
        to: Some(x.clone())}, &(2 as usize));
// Resource was moved from `y` to `x`
    vd.append_external_event(ExternalEvent::Move{from: Some(x.clone()),
        to: Some(y.clone())}, &(3 as usize));
// Resource of `x` is dropped
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: x }, &(5 as usize));
// Resource of `y` is dropped
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: y }, &(5 as usize));
```
Now the final step is to activte the rendering function that generate the [vis_code.svg]() and [vis_timeline.svg]() that are visulization SVG files for the code section and timeline section using the `svg_generation::render_svg()` function:
```
svg_generation::render_svg(&"examples/string_from_move_print/input/".to_owned().to_owned(), &"examples/string_from_move_print/".to_owned(), & mut vd);
```
Phew! Good Work! What's left is to run the program. Direct into the */svg_generator* folder and run
```
cargo run --example string_from_move_print
```
Now your folder should look like this:
```
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
├── source.rs
├── vis_code.svg
└── vis_timeline.svg
```
Congratulations! You have Successfully generated the visulizations! Add the name of your example folder to */test_example/test_examples.sh* and see them in action.

## Data Structures and Function Specifications
Yet to be finished....

## Modules
1. [mdbook_plugin](mdbook_plugin)

    a. [book.js](mdbook_plugin/book.js):

    | Relevant Lines | Purpose |
    | ---            | :---    |
    | 18-42   | `adjust_visualization_size()`: Responsible for automatically resizing visualization flexboxes on page load. |
    | 228-283 | Responsible for adding toggle buttons to every code block that contains a corresponding visualization. |

    b. [helpers.js](mdbook_plugin/helpers.js): responsible for dynamic/interactive portions of the visualization, from hover messages to word highlighting.

    c. [visualization.css](mdbook_plugin/visualization.css.js): defines page's flexbox styling

2. [svg_generator](svg_generator)

    a. [examples](svg_generator/examples): contains all examples to be rendered

        Folder structure for new examples:
            <example_name>
            ├── input
            │   └── annotated_source.rs
            ├── main.rs
            ├── source.rs
            ├── vis_code.svg
            └── vis_timeline.svg

    | File                  | Purpose   |
    | :----:                | :-----    |
    | `annotated_source.rs` | Adds styling to code panel with the use of &lt;tspan&gt; tags<br>Properties of Variables: `data-hash`<br>Properties of Functions: `hash`, `data-hash="0"`, `class="fn"`     |
    | `main.rs`             | Defines all ResourceAccessPoint types and events |
    | `source.rs`           | Contains original, source code that will be rendered into SVG  |
    | `vis_code.svg`         | (1/2) Final rendered SVG code panel   |
    | `vis_timeline.svg`     | (2/2) Final rendered SVG timeline panel with arrows, dots, etc |

    b. [src](svg_generator/src)

    | File                  | Purpose   |
    | :----:                | :-----    |
    | [data.rs](svg_generator/data.rs) | Defines all ResourceAccessPoint types and is responsible for calculating transition between states |
    | [hover_messages.rs](svg_generator/hover_messages.rs) | Contains all hover message templates |
    | [code_panel.rs](svg_generator/src/code_panel.rs)<br>[code_template.svg](svg_generator/src/code_template.svg) | Defines template for code panel and builds corresponding SVG renderings |
    | [timeline_panel.rs](svg_generator/src/timeline_panel.rs)<br>[timeline_template.svg](svg_generator/src/timeline_template.svg) | Defines template for timeline panel and builds corresponding SVG renderings |
    | [svg_generation.rs](svg_generator/src/svg_generation.rs) | Renders source code to SVG images and saves them under respective directory in `svg_generator/examples/` |
    | [line_styles.rs](svg_generator/src/line_styles.rs) | Unused |
    
## Visualization Limitations
Yet to be finished....
