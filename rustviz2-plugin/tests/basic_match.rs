pub enum Fruit {
  Apple(u32),
  Banana
}

fn main () {
  let mut x = Fruit::Apple(8);
  let mut y = Fruit::Banana;
  match x {
    Fruit::Apple(z) => { 
      y = Fruit::Apple(z)
    }
    _ => {
      y = Fruit::Banana
    }
  };
}