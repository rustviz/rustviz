struct appy {
  a: u32
}


fn main() {
    let mut x = 5;
    x += 5;

    let mut y = String::from("huh");
    y = String::from("huh2");

    let z = & mut x;
    let y = &String::from("");
    let z = y;
    let z2 = & appy {a: 4};


}