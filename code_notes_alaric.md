## fn render_code_panel

+ `max_x_space`: maximum number of characters of a single line in `source.rs`

+ `code_panel_string`: SVG filled up content after `fn render_code_panel` is done

+ `num_lines`: number of lines in code panel

+ `y = y + LINE_SPACE`: each new row/line is incremented by `LINE_SPACE = 30` units. (**the same goes for timeline panel**)

+ line 49, shows how to render template string to svg.

```rust
 output.push_str(&handlebars.render("code_line_template", &data).unwrap());
```

 1. `handlebars` was registered template as:

```rust
let line_template =
    "        <text class=\"code\" x=\"{{X_VAL}}\" y=\"{{Y_VAL}}\"> {{LINE}} </text>\n";
// register the template. The template string will be verified and compiled.
assert!(handlebars
    .register_template_string("code_line_template", line_template)
    .is_ok());
```

2. each field of template is set up and pushed into `BTreemap`. Later use deserialize method to render as string:

```rust
let mut data = BTreeMap::new();
data.insert("X_VAL".to_string(), x.to_string());
data.insert("Y_VAL".to_string(), y.to_string());
/* automatically add line numbers to code */
let fmt_line = format!(
    "<tspan fill=\"#AAA\">{}  </tspan>{}",
    line_of_code, line_string
);
data.insert("LINE".to_string(), fmt_line);
```

## Struct VisualizationData

+ `event_line_map`: 

```rust
 pub event_line_map: BTreeMap<usize, Vec<ExternalEvent>>
```

1. stores events that has something to do with events relating two `ResourceAccessPoint` (two variables, but not function)

2. in SVG, corresponds two horizontal arrow, e.g, ownership transfer

![](/Users/alaric66/Library/Application%20Support/marktext/images/2023-07-02-17-31-36-image.png)

## fn vec_to_map

+ calculate `hash` for each RAP.

+ `hash` is the sequence order of RAP appearing in `"/* --- BEGIN Variable Definitions ---"`

+ **since each RAP contains `hash` themselves, and RAPs will be further moved into `VisualizationData`. Hashes will be synchronized in rendering code panel and timeline panel.**
  
  - [ ] What's the usage of `hash`? Still don't know yet. It seems the line mirroring between code and timeline panel doesn't need the help of `hash`.
    
    - [x] I think `hash` is more like a tag for each RAP, so it's easier to make a mapping.
  
  - [ ] `data-hash` aspect of the SVG seems to be predefined by `annotated_source.rs`, which has nothing to do with `hash`

## fn prepare_registry : timeline_panel.rs

+ defines all handlebar templates inside SVG

## fn compute_column_layout : timeline.rs

+ column corresponds to variables with visualization, namely, how their labels are dispersed in space. It's done by calculating each column's `x` axis.
  
  + by default, columns are separated by 70 units. But if var label name is longer, it will grow to `var.len() * 13 - 13`.
  
  + structs are given special attention. the following tries to group struct and its member variable together (to render struct box)
    
    + updated in `structs_info: &'a mut StructsInfo`
  
  ```rust
  if (owner == -1) && timeline.resource_access_point.is_struct_group() && !timeline.resource_access_point.is_member() {
      owner = timeline.resource_access_point.hash().clone() as i64;
      owner_x = x;
  } else if (owner != -1) && timeline.resource_access_point.is_struct_group() && timeline.resource_access_point.is_member() {
      last_x = x;
  } else if (owner != -1) && !timeline.resource_access_point.is_struct_group() {
      structs_info.structs.push((owner, owner_x, last_x));
      owner = -1;
      owner_x = 0;
      last_x = 0;
  }
  ```

+ return value `(resource_owners_layout, (x as i32)+100)`. It means the width of SVG is the second field.

+ `resource_owners_layout: BTreeMap<&u64, TimelineColumnData>`: stores mapping from hash of RAP to TimelineColumnData



## struct TimelineColumnData

+ main information of column visualization

```rust
struct TimelineColumnData {
    name: String,
    x_val: i64,    // column starts at SVG x coordinate = x_val
    title: String,    // e.g, x, immutable
    is_ref: bool,
    is_struct_group: bool,
    is_member: bool,
    owner: u64    // if it's struct member, point to struct "owner" hash;
// otherwise, hash of itself
}
```

+ `TimelineColumnData.owner` will always be -1 if it's not struct related
  
  + it will be the hash to struct owner type if it's struct related









## hover_messages.rs

+ contains useful functions to generate hover message source strings.



# Instructions on rendering lifetime body
+ Use `timeline_panel.rs`.
  + In `fn prepare_registry`, there are several templates we can use:
    + `vertical_line_template` for solid line => lifetime body


## TODO
+ line number parsing (input line number is absolute line number, whereas rustviz will relative line number w.r.t main())
  + need to transform it to relative line number