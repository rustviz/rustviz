fn main() {
    let s = String::from("hello");
    takes_ownership(s);
    let mut x = 5;
    let y = x;
    x = 6;
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}

fn mut_life<'i,'a>(s: &'i i32, x: &'a u32, z: &'a Vec<Option<String>>) -> (&'i u32, &'a VecDeque<(u32, Vec<i32>)>){

}