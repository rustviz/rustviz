### Optional: Structs in Rust

#### Creating a struct

To define a struct, we enter the keyword `struct` and name the entire struct. A struct’s name should describe the significance of the pieces of data being grouped together. Then, inside curly brackets, we define the names and types of the pieces of data, which we call *fields*. Here is an example showing a struct that stores information about a user account.

```rust
struct User {
    username: String,
    email: String,
    sign_in_count: u64,
    active: bool,
}
```

We create an instance by stating the name of the struct and then add curly brackets containing `key: value` pairs, where the keys are the names of the fields and the values are the data we want to store in those fields.  Then we can use dot field to obtain the value in a struct.

```rust
    let mut user1 = User {
        email: String::from("someone@example.com"),
        username: String::from("someusername123"),
        active: true,
        sign_in_count: 1,
    };

    user1.email = String::from("anotheremail@example.com");
```
Each fields in the struct can be referenced independently. Here's an example of defining a struct, generating an instance of it, letting it interact with functions and referencing field `r.h`.

```rv
struct Rect {
    w: u32,
    h: u32,
}

fn main() {
    let r = Rect {
        w: 30,
        h: 50,
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        area(&r)
    );
    
    println!("The height of that is {}.", r.h);
}

fn area(rect: &Rect) -> u32 {
    rect.w * rect.h
}
```

#### Calling a method in a struct

Struct can also include methods whose definition is given in the `impl` of it.  When calling a method or a variable from a struct, we use `object.something()`or ` (&object).something()`, which are the same. No matter it is a `&, &mut, *`or nothing, always use `.` and not need to use `->` because Rust will automatically adds in `&, &mut, *` so `object` matches the signature of the method. 

```rust
struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn area(&self) -> u32 {
        self.width * self.height
    }
}

fn print_area(rect: &Rectangle) {
    println!(
        "The area of the rectangle is {} square pixels.",
       	rect.area() // dot even though it's actually a reference
    );
}

fn main() {
    let r = Rectangle {
        width: 30,
        height: 50,
    };

    print_area(&r);
}
```

#### Ownership of struct data

When the instance of the struct owns all its fields, i.e. no reference or pointer in the struct, the ownership is basically the same with data outside of a struct. It's also possible for fields of a struct to own resources. Here's an example of the cases where one of the field `y` owns a `string` resouce.

```rv
struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let _y = String :: from("bar");
    let f = Foo { x: 5, y: _y };
    println!("{}", f.x);
    println!("{}", f.y);
}
```

When the any of the data members is not owned by the struct, it needs lexical lifetime specified to allow the struct owning a reference of a data resouce. This will ensure that the resource referenced will have the same lifetime as the struct as long as they share the same lexical lifetime label.

Here is an example of using lifetime annotations `<'a>` in struct definitions to allow reference of string `&p` in a `struct Excerpt`.

```rust
struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from("Ok. I'm fine.");
    let first = n.split('.').next().expect("Could not find a '.'");
    let i = Excerpt {
        p: first,
    };
    println!("{}", first);
    // 'i' cannot be returned be returned
    // because the struct outlives 'n'
}

fn main() {
    some_function();
}
```