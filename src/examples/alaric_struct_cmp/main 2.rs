struct MagicBox<'i>{
    num: &'i u32,
    id: String
  }
  
  impl<'i> MagicBox<'i>{
    fn max(&'i self, other: &'i u32) -> &'i u32{
        if self.num > other{
            self.num
        }
        else{
            other
        }
    }
  }
  
  
  fn main(){                    // 1
      let x : u32 = 10;         // 2
      let y : u32 = 16;         // 3
      let mb = MagicBox{        // 4
        num : &x,               // 5
        id: String::from("box") // 6
      };                        // 7
      let val = mb.max(&y);     // 8
      println!("bigger one is {}", val);    // 9
  }