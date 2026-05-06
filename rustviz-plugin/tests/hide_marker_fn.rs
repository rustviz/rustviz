// `// rustviz: hide` on a fn signature removes the entire fn from
// the rendered code panel and bypasses body traversal — but call
// sites in non-hidden fns still emit their move arrow into the
// hidden fn's Function RAP.

fn main() {
    let s = String::from("hi");
    helper(s);
}

fn helper(some_string: String) { // rustviz: hide
    println!("{}", some_string);
}
