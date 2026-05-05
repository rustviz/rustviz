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
  let mut y = Fruit::Banana;
  match x {
    Fruit::Apple(z) => { 
      y = Fruit::Apple(z);
    }
    Fruit::Grape  => {
      y = Fruit::Banana
    }
    Fruit::Banana => {
      y = Fruit::Banana
    }
    Fruit::Strawberry => {
      y = Fruit::Banana
    }
    Fruit::Guava => {
      y = Fruit::Banana
    }
    Fruit::Bean => {
      y = Fruit::Banana
    }
  };
}