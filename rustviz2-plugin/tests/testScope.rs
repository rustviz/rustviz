fn main() {
    let s1=String::from("Hello"); 
    let s2={
      let s3=String::from("World");
      s3
    }; 
    println!("{}",s1);
    //println!("{}",s2);
  }  