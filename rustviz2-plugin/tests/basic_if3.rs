fn main () {
    let mut x = String::from(" hello ");

    if true {

      take_ownership(x);
    }
    else {

      x.push('c');
    }

    println!("");
}

fn take_ownership(s: String) {
  
}