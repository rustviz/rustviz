struct MagicBox<'i>{
    num: &'i u32,
    id: String
  }
  
  impl<'i, 'a> MagicBox<'i,'a>{
    fn conv(&'i self, other: &'i u32, thr: &'a Vec<int>) -> &'i u32{
        if self.num > other{
            self.num
        }
        else{
            other
        }
    }
  }
  
  fn main(){                  // 1
    let x : u32 = 10;         // 2
    let y : u32 = 16;         // 3
    let mb = MagicBox{        // 4
      num : &x,               // 5
      id: String::from("box") // 6
    };                        // 7
    let v : Vec<int> = Vec::new();
    let val = mb.conv(&y, &v);     // Lifetime
    println!("bigger one is {}", val);    // 9
}