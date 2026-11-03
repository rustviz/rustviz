// Chained field assignment of a non-Copy value (#143): the field's old
// String is dropped at the write, and `s` is moved into the field.
struct Name {
    first: String,
}

struct Person {
    name: Name,
}

fn main() {
    let mut p = Person { name: Name { first: String::from("Ada") } };
    let s = String::from("Grace");
    p.name.first = s;
    println!("{}", p.name.first);
}
