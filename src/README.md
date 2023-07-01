# Dev Doc
*This is a Dev Documents for people who wants to contribute to this project.*

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
        Struct(Struct),
    }
    ```
    - Owner<a name="Owner"></a>
    For the owner of a resource, we need to define several properties: The name of the variable, the hash number and whether the vairable is mutable. The *lifetime_trait* property is not yet implemented.
        ```
        pub struct Owner {
            pub name: String,
            pub hash: u64,
            pub is_mut: bool, // let a = 42; vs let mut a = 42;
        }
        ```
    - Struct<a name="Struct"></a>
    For the owner and members of a struct, we need to define several properties: The name of the variable, the hash number of itself and its owner, if it is a member and whether the vairable is mutable. The *lifetime_trait* property is not yet implemented.
        ```
        pub struct Struct {
            pub name: String,
            pub hash: u64,
            pub owner: u64, // if it is the owner, then keep it the same as hash of itself
            pub is_mut: bool, // let a = 42; vs let mut a = 42;
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
            pub is_mut: bool,
        }
    
        // a reference of type & T
        #[derive(Clone, Hash, PartialEq, Eq, Debug)]
        pub struct StaticRef {                // let (mut) r1 = & a;
            pub name: String,
            pub hash: u64,
            pub is_mut: bool,
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
    | [line_styles.rs](svg_generator/src/line_styles.rs) | Unused |

