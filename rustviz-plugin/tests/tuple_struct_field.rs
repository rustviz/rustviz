// Reading and writing a tuple struct's fields (`black.0`). Tuple
// structs get a column per field like named-field structs. On main
// before #141 any `x.0` projection panicked in annotated_source_gen
// (the Field arm unwrapped an id_map lookup for the field name).

struct Color(u8, u8, u8);

fn main() {
    let mut black = Color(0, 0, 0);
    black.1 = 10;
    let r = black.0;
}
