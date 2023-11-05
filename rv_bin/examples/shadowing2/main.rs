/* --- BEGIN Variable Definitions ---
Owner x;
Owner x;
Function String::from();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
    {
      let x = x * 6; // !{ Bind(x) }
      println!("{}", x); // !{ PassByStaticReference(x->println!())  }
    } // !{ GoOutOfScope(x) }
    println!("{}", x); // !{ PassByStaticReference(x->println!())  }
} // !{ GoOutOfScope(x) }