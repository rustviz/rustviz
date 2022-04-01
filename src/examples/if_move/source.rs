fn main() {
    let x = String::from("ABC");
    let guard = 1
    if guard == 1 {
        takes_ownership(x);
    }
    else {
        0
    }
}


fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}