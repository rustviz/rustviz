/* --- BEGIN Variable Definitions ---
Owner x;
Owner x;
Function String::from();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    {
      let x = String::from("world"); // !{ Move(String::from()->x)}
      println!("{}", x); // !{ PassByStaticReference(x->println!())  }
    } // !{ GoOutOfScope(x) }
    println!("{}", x); // !{ PassByStaticReference(x->println!())  }
} // !{ GoOutOfScope(x) }