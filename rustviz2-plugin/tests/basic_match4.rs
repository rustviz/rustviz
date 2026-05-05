pub enum Fruit {
  Apple(u32),
  Banana,
  Grape,
  Strawberry,
  Guava,
  Bean
}

fn main () {
  let mut x = Fruit::Apple(8);
  match x {
    Fruit::Apple(z) => { 
      x = Fruit::Apple(z);
      match x {
        Fruit::Apple(c) => {
            x = Fruit::Apple(c + 1);
        }
        _ => {
          x = Fruit::Banana;
        }
      }
    }
    Fruit::Banana => {
  
    }
    _ => {
      x = Fruit::Bean;
    }
  }
}