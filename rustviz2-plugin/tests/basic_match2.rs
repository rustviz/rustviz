#[derive(Debug)]
pub enum Fruit {
  Apple(u32),
  Banana,
  Grapes(String)
}

fn f() -> String {
  let h = String::from("hello");
  h
}

fn main () {
  let mut x = Some(String::from("hello"));
  let mut y = Fruit::Banana;
  match (& mut x, y) {
    (Some(s), c) => {
      s.push_str(" world");
      println!("s {}", s);
    }
    z => {
    
    }
  }

  // println!("{:#?}", x);
}