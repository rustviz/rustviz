// #396: a write through a reference (or Box) stored in a collection
// goes into what the element points to, not into the collection.
// `*slots[0] += 10` changes x through the `&mut i32` in slots[0];
// slots is a Vec, not a reference. Which variable an element points
// to isn't known statically, so the write is drawn on slots's column
// and names the element.
struct Holder<'a> {
    r: &'a mut i32,
}

fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut slots = vec![&mut x, &mut y];
    *slots[0] += 10;
    *slots[1] = 7;
    let k = 8;
    *slots[1] = k;
    println!("{:?}", slots);
    let mut a = String::from("a");
    let mut names = vec![&mut a];
    let s = String::from("c");
    *names[0] = s;
    *names[0] = String::from("d");
    println!("{:?}", names);
    let mut z = 3;
    let mut hs = vec![Holder { r: &mut z }];
    *hs[0].r += 1;
    let mut boxes = vec![Box::new(1)];
    *boxes[0] = 5;
    println!("{} {}", hs.len(), boxes[0]);
}
