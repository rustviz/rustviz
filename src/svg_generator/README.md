# RustViz
*RustViz* is a tool written in Rust that generates visualizations from simple Rust programs to assist potential users and students in better understanding the Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) mechanism.

## Documentation
* [Example Usage](#Example-Usage)
* [User Define Usage](#User-Define-Usage)
* [Data Structure and Function Specifications](#Data-Structures-and-Function-Specifications)
* [Modules](#Modules)
* [Visulization Limitations](#Visualization-Limitations)

## What does it look like? 

*RustViz* generates *.svg* files of graphical indicators that integrate with [mdbook](https://github.com/rust-lang/mdBook) to generate visualization over user-defined rust code examples. Here's a sample view of what the visualization looks like:

![alt tag](https://github.com/rustviz/rustviz/blob/master/src/svg_generator/example.png)

## Example Usage
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

## User Define Usage
In this section, we'll take a look into how to create example by using our example [string_from_move_print](svg_generator/examples/string_from_move_print). let's first take a look at the file structure you need for the example to run:
```
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
└── source.rs
```
let's first take a look at the [source.rs](svg_generator/examples/string_from_move_print/source.rs), which is simply the rust source code that we are generating visulization from:
```
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y)
}
```
In this example, the string `hello`'s resource is first moved from `String::from()` to `x`, then `x`'s resource is moved to `y`. Lastly, we print the value by taking `y` as an input to `println!()` but the resource has not been moved. 

Next, let's focus on we need to do in [main.rs](svg_generator/examples/string_from_move_print/main.rs). In this visuliation tool, **we define all possible owners, references or input of any memory resource as a** [Resource Access Point](#ResourceAccessPoint). In this case, we have the function `String::from()` and two variables `x` and `y` as Resource Access Points. Correspondingly in our implementation, the [Resource Access Point](#ResourceAccessPoint) is defined as an enum that hold the possible types of Resource Access Points, namely `ResourceAccessPoint::Owner` and `ResourceAccessPoint::Function` in this case. We want to create instance that represent these functions and variables in our main program:
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
Next we decalre an instance of the VisualizationData struct as a container that holds all the information of [ExternalEvent](#ExternalEvents) that we will talk about up next, all you need is to declare the struct instance without any modification:
```
let mut vd = VisualizationData {
    timelines: BTreeMap::new(),
    external_events: Vec::new(),
    preprocess_external_events: Vec::new(),
    event_line_map: BTreeMap::new()
};
```
The [ExternalEvent](#ExternalEvents) **is an enum that hold all the movement, borrowing and dropping of a resource.** In our case, we have four of such event: 
1. Resource was moved from `String::from()` to `x`
2. Resource was moved from `y` to `x`
3. Resource of `x` is dropped
4. Resource of `y` is dropped

We then add these events information to the VisualizationData instance we declared before by using the `append_external_event()` function:
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
Now the final step is to activte the rendering function that generate the vis_code.svg and vis_timeline.svg that are visulization SVG files for the code section and timeline section using the `svg_generation::render_svg()` function:
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
│   └── annotated_source.rs
├── main.rs
├── source.rs
├── vis_code.svg
└── vis_timeline.svg
```
Congratulations! You have Successfully generated the visulizations! Add the name of your example folder to */test_example/test_examples.sh* and see them in your browser.

## Data Structures and Function Specifications
* [Resource Access Point](#ResourceAccessPoint)
    * [Owner](#Owner)
    * [Struct](#Struct)
    * [Mutable reference and Inmutable reference](#mutablereferenceandinmutablereference)
    * [Functions](#Functions)
* [External Events](#ExternalEvents)
    * [Duplicate](#Duplicate)
    * [Move](#Move)
    * [Static Borrow](#StaticBorrow)
    * [Mutable Borrow](#MutableBorrow)
    * [Static Return](#StaticDie)
    * [Mutable Return](#MutableDie)
    * [Pass By Static Reference](#PassByStaticReference)
    * [Pass By Mutable Reference](#PassByMutableReference)
    * [Go Out Of Scope](#GoOutOfScope)
    * [Initialize Param](#InitRefParam)
- [ResourceAccessPoint](svg_generator/src/data.rs) <a name="ResourceAccessPoint"></a>
ResourceAccessPoint is an enum that define all possible owner, references or creator of any memory resource. For now, the types of ResourceAccessPoint could possibly be an owner of a resource, a mutable reference of a resource, a unmutable referene of a resource or a function:
    ```
    pub enum {
        Owner(Owner),
        MutRef(MutRef),
        StaticRef(StaticRef),
        Function(Function),
    }
    ```
    - Owner<a name="Owner"></a>
    For the owner of a resource, we need to define several properties: The name of the variable, the hash number and whether the vairable is mutable. The *lifetime_trait* property is not yet implemented.
        ```
        pub struct Owner {
            pub name: String,
            pub hash: u64,
            pub is_mut: bool, // let a = 42; vs let mut a = 42;
            pub lifetime_trait: LifetimeTrait,
        }
        ```
    - Struct<a name="Struct"></a>
    For the owner and members of a struct, we need to define several properties: The name of the variable, the hash number of itself and its owner, if it is a member and whether the vairable is mutable. The *lifetime_trait* property is not yet implemented.
        ```
        pub struct Owner {
            pub name: String,
            pub hash: u64,
            pub owner: u64, // if it is the owner, then keep it the same as hash of itself
            pub is_mut: bool, // let a = 42; vs let mut a = 42;
            pub lifetime_trait: LifetimeTrait,
            pub is_member: bool, 
        }
        ```
    - Mutable reference and Inmutable reference<a name="mutablereferenceandinmutablereference"></a>
    The defintion for references are similar to that of a Owner, but additionally we need to define the *my_owner_hash*, which refer back to the hash number of its owner. We also need to define *is_mut*, which represent the mutability of the reference. The *lifetime_trait* property is not yet implemented.
        ```
        // a reference of type &mut T
        #[derive(Clone, Hash, PartialEq, Eq, Debug)]
        pub struct MutRef {         // let (mut) r1 = &mut a;
            pub name: String,
            pub hash: u64,
            pub my_owner_hash: Option<u64>,
            pub is_mut: bool,
            pub lifetime_trait: LifetimeTrait,
        }
    
        // a reference of type & T
        #[derive(Clone, Hash, PartialEq, Eq, Debug)]
        pub struct StaticRef {                // let (mut) r1 = & a;
            pub name: String,
            pub hash: u64,
            pub my_owner_hash: Option<u64>,
            pub is_mut: bool,
            pub lifetime_trait: LifetimeTrait,
        }
        ```
    - Functions<a name="Functions"></a> 
    For each function, we only need to specify its name and hash number.
        ```
        pub struct Function {
            pub name: String,
            pub hash: u64,
        }
        ```
    
- [ExternalEvents](svg_generator/src/data.rs) <a name="ExternalEvents"></a>
ExternalEvents is an enum that hold all the movements of a the resource, here is the list of all the possible movements are avaliable for visualization:
    - Duplicate <a name="Duplicate"></a>
        The Duplicate event represent the copy of one variable to the other that does not involve the move of resource.
        ```
        Duplicate {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        let y = 5; // Duplicate from None to y 
        // set from Option to None to represent initialization
        let x = y; // Duplicate from y to x
        ```
    - Move <a name="Move"></a>
    The Move event represent the tranferring of resource from one detination to the other.
        ```
        Move {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        let x = String::from("Hello"); // Move from String::from() to x
        let y = x; // Move from x to y
        ```
    - StaticBorrow <a name="StaticBorrow"></a>
    The StaticBorrow event represent the immutable borrowing in rust
        ```
        StaticBorrow {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        let x = String::from("hello");
        let y = &x; // immutable borrow from x to y
        ```
    - MutableBorrow <a name="MutableBorrow"></a>
    The MutableBorrow event represent the mutable borrowing in rust
        ```
        MutableBorrow {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        let mut x = String::from("Hello");
        let y = &mut x; // mutable borrow from x to y
        ```
    - StaticDie <a name="StaticDie"></a>
    The StaticDie event represent return of a unmutably borrowed source.
        ```
        StaticDie {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        fn main() {
            let z = &mut x;
            world(z); // return mutably borrowed source from z to x since z is no longer used
        }
        fn world(s : &mut String) { 
            s.push_str(", world")
        }
        ```
    - MutableDie <a name="MutableDie"></a>
    The MutableDie event represent return of a mutably borrowed source.
        ```
        MutableDie {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>,
        },
        ```
        User case:
        ```
        fn main() {
            let y = &x
            let z = &x;
            f(y, z); // return immutably borrowed source from z to x since z is no longer used
            // also return immutably borrowed source from y to x since y is no longer used
        }
        fn f(s1 : &String, s2 : &String) { 
            println!("{} and {}", s1, s2)
        }
        ```
    - PassByStaticReference <a name="PassByStaticReference"></a>
    The PassByStaticReference event represent passing an inmutable reference to a function.
        ```
        PassByStaticReference {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>, // must be a function
        },
        ```
        User case:
        ```
        fn main() {
            let x = String::from("hello"); 
            f(&x); // f() could only read from x
        }
        fn f(s : &String) { 
            println!("{}", s) 
        } 
        ```
    - PassByMutableReference <a name="PassByMutableReference"></a>
    The PassByMutableReference event represent passing a mutable reference to a function.
        ```
        PassByMutableReference {
            from: Option<ResourceAccessPoint>,
            to: Option<ResourceAccessPoint>, // must be a function
        },
        ```
        User case:
        ```
        fn main() {
            let z = &mut x;
            world(z); // world() could read from/write to z
        }
        fn world(s : &mut String) { 
            s.push_str(", world")
        }
        ```
    - GoOutOfScope <a name="GoOutOfScope"></a>
    The GoOutOfScope event represent a variable go out of scope.
        ```
        GoOutOfScope {
            ro: ResourceAccessPoint // must be a variable
        },
        ```
        User case:
        ```
        fn main() { 
            let x = 5; 
            let y = x; // x and y both go out of scope
        } 
        ```
    - InitRefParam <a name="InitRefParam"></a>
    The InitRefParam event represent initialization of the parameters within a function
        ```
        InitRefParam {
            param: ResourceAccessPoint, // the parameter in function
        }
        ```
        User case:
        ```
        fn takes_ownership(some_string: String) { // initialize some_string
            println!("{}", some_string) 
        } 
        ```
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
    | [line_styles.rs](svg_generator/src/line_styles.rs) | Defines state for vertical line components as they relate to Owners |

## Understanding How Rustviz Works
It is recommended that you read over the content above and investigate the source files before attempting to understand the following section.

1. <b>Parsing main.rs</b>
    - `parse_vars_to_map`
        Parses the block comments at the beginning of the main.rs file that refer to variable definitions  and constructs a map from the name of a variable to its respective [Resource Access Point](#ResourceAccessPoint). Each variable is assigned a distinct hash that increases incrementally such that interactions in the visualization appear in the order by which they were defined. For example, the following variable definitions would result in the ordering `[s, r1, r2, r3]` of timelines (left to right).
        ```
            /* --- BEGIN Variable Definitions ---
            Owner mut s; StaticRef r1; StaticRef r2; MutRef r3;
            --- END Variable Definitions --- */
        ```
    - `extract_events`
        Builds a vector of `(line number, event)` tuples by parsing the comments of each line in the main.rs file. Note that at this moment said vector only contains the string representations of each event and not [External Events](#ExternalEvents).
        ```
            let r1 = &s; // !{ StaticBorrow(s->r1) }
            let r2 = &s; // !{ StaticBorrow(s->r2) }

            events: [(4, "StaticBorrow(s->r1)"), (5, "StaticBorrow(s->r2)")]
        ```
2. <b>Building Visualization Data </b>
    - `add_events`
        Constructs an [External Event](#ExternalEvents) for each respective string representation found in the vector returned by `extract_events`. These events are paired in `(line number, event)` tuples that are appended to the `preprocess_external_events` member of the `vd` VisualizationData struct. The hashmap returned from `parse_vars_to_map` is utilized to confirm that each event contains only variables, functions, or structs that were defined in the variable definition block comments at the top of main.rs. The member function `append_external_event` will insert a `{line, ExternalEvent}` key-value pair in the `event_line_map` member of the `vd` struct.
        ```
            match field[0] {
                "Bind" => vd.append_external_event(
                    ExternalEvent::Bind{
                        from: get_resource(&vars, "None"),
                        to: get_resource(&vars, field[1])
                    }, &(event.0 as usize)
                ),
        ```
        In the case of `Bind` a 'dummy' [RAP](#ResourceAccessPoint) is used for the `from:` field. 
3. <b>Rendering svg files</b>
    - `render_svg`
        First, each vector of [External Events](#ExternalEvents) in the `event_line_map` is sorted in the order specified by their corresponding unique hashes. This allows for events to be described in any order in the comments of main.rs irrespective of the order defined originally in the header comments, granting the user more flexibility. For example:
        ```
            //main.rs
            // !{ GoOutOfScope(s), GoOutOfScope(r1), GoOutOfScope(r2), GoOutOfScope(r3) }
    
            Is equivalent to
    
            // !{ GoOutOfScope(r1), GoOutOfScope(s), GoOutOfScope(r3), GoOutOfScope(r2) }
        ```
        Then, line numbers corresponding to [External Events](#ExternalEvents) need to be transposed to their final position as the line numbers extracted from main.rs are not representative of the line numbers in the visualization. Additionally, the timelines, member of `vd` is initialized with the `_append_event` function.
      ```
          pub struct VisualizationData {
            //      timelines: an orderred map from a Variable's hash to the Variable's Timeline.
            pub timelines: BTreeMap<u64, Timeline>,
      ```
      A variable's timeline holds all interactions that it engages in (as it relates to ownership) that will appear in the visualization.
      ```
          pub struct Timeline {
            pub resource_access_point: ResourceAccessPoint,    // a reference of an Owner
            // line number in usize
            // a vector of ownership transfer history of a specific variable, in a sorted order by line number.
            pub history: Vec<(usize, Event)>,
          }
      ```
   - Handlebars
        At this point all the necessary data has been extracted and processed from the main.rs file and is ready to be interpolated into SVG elements. Here is an example of an SVG element (a hoverable dot) and its corresponding line of code:
        ```
            <circle cx="70" cy="115" r="5" data-hash="1" class="tooltip-trigger" data-tooltip-text="&lt;span style=&quot;font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;&quot;&gt;s&lt;/span&gt; acquires ownership of a resource"/>

            let mut s = String::from("hello");

        ```
        Two svg files are rendered for the final visualization: the code panel (the text) `vis_code.svg` and the timeline panel (the interactive state diagram) `vis_timeline.svg`. Templates for each component of the visualization: dots, arrows, vertical lines, etc are registered using Handlebars and are utilized to dynamically render visualizations. Here is the dot template that is used to generate the SVG element in the example above:
        ```
            let dot_template = 
            "        <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\" data-hash=\"{{hash}}\"     class=\"tooltip-trigger\"data-tooltip-text=\"{{title}}\"/>\n";

            assert!(
                registry.register_template_string("dot_template", dot_template).is_ok()
            );
        ```
        Functions in `timeline_panel.rs` translate state from the VisualizationData, `vd`, struct into the content that will satisfy the parameters of the different component templates. Below is a snippet of code from the `create_owner_line_string` function that demonstrates how the vertical line corresponding to a mutable variable is rendered.
        ```
            let style = determine_owner_line_styles(rap, state);
            match (state, style) {
                (State::FullPrivilege, OwnerLine::Solid) | (State::PartialPrivilege{ .. }, OwnerLine::Solid) => {
                    data.line_class = String::from("solid");
                    data.title += ". The binding can be reassigned.";
                    registry.render("vertical_line_template", &data).unwrap()
            },
        ```
        Once each component of the visualization is rendered, contents of both the code panel and timeline panel are written to `vis_code.svg` and `vis_timeline.svg` respectively, which at that point are ready to be integrated into the rustviz-tutorial mdbook. 
