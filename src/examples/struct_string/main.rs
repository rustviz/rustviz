/* --- BEGIN Variable Definitions ---
Struct f{x,y};
Owner _y;
Function String::from();
Function println!();
--- END Variable Definitions --- */
struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let _y = String :: from("bar"); // !{ Move(String::from()->_y) }
    let f = Foo { x: 5, y: _y }; // !{ Bind(f), Bind(f.x), Move(_y->f.y) }
    println!("{}", f.x); // !{ PassByStaticReference(f.x->println!())  }
    println!("{}", f.y); // !{ PassByStaticReference(f.y->println!())  }
} // !{ GoOutOfScope(f), GoOutOfScope(f.x), GoOutOfScope(f.y) }