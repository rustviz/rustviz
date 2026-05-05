# Vectors in Rust
The previous sections cover everything you need to know about ownership and borrowing in Rust! This section introduces another interesting data structure: vectors.

Like with other languages, the Rust standard library contains many useful
*collection* types. One of the most useful and common ones are *vectors*, which
have type `Vec<T>`, where `T` is the type the that vector holds.

Vectors are heap-allocated, mutable collections that store multiple values of
the same type contiguously in memory. In many ways, they are similar C++
`vector`s and serve similar purposes.

Vectors are implemented with *generics*, which allow them to hold any type.
For example, we can have `Vec<i32>` and `Vec<String>` which are the types of
`i32` vectors and `String` vectors, respectively. Vectors can hold any
`struct` or `enum` type as well. 

## Creating A Vector

### Empty Vectors
To make a new empty vector, we can use the `Vec::new()` function as follows:

```rust
fn main() {
    let v: Vec<i32> = Vec::new();
}
```

Here, `Vec::new()` creates an empty vector of `i32`s and moves ownership to `v`.
Note that we included a type annotation to `v`. This is necessary here because
otherwise, Rust won't know which type of vector to create. 

### Creating Vectors from Initial Values
We can also create new vectors with initial values using the `vec!` macro:

```rust
fn main() {
    let v = vec![1, 2 ,3];
}
```

Here, we create a new `Vec<i32>` containing the values `1`, `2`, and `3` in
that order. Note that in this case, we did not need to include a type annotation
for `v`. This is because we are creating the vector with initial values of a
specific type, so Rust can figure out the type of `v` in this case.

## Reading Elements of Vectors

### Accessing an Element at a Particular Index
We can use the indexing syntax or the `get()` method to get the value at a
particular index of the vector:

```rust
fn main() {
    let v = vec![1, 2, 3];

    let third: &i32 = &v[2];
    println!("The third element is {}", third);

    match v.get(2) {
        Some(third) => println!("The third element is {}", third),
        None => println!("There is no third element."),
    }
}
```

Here, we use both ways of getting a particular element. The first way is using
the indexing syntax (square brackets), which gives us an immutable reference to
the element. The second way is using the `get()` method, which returns an
`Option` type. 

With the indexing syntax, if we performed an out-of-bounds access in the vector,
the program would *panic* (i.e. cause an unrecoverable error.) With the `get()`
method, an out-of-bounds access would result in the method returning `None`.
With the `get()` method, we can handle out-of-bounds accesses gracefully rather
than causing the program to crash. 

### Iterating over Elements
We can iterate over elements in a vector with a `for` loop to read the values:

```rust
fn main() {
    let v = vec![1, 2, 3];
    for i in &v {
        println!("{}", i);
    }
}
```

Here, we simply read the values of the vector and print them to the terminal.
Note that the `for` loop is immutably borrowing `v`, as shown by the `&v`.


## Mutating Vectors

### Push
We can add elements to the back of a vector using the `push()` method:

```rust
fn main() {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
}
```

This creates an empty vector and adds the values `1`, `2`, and `3` to the back
of the vector in that order. In this case, we did not need a type annotation
because the type is inferred from the values we pushed to it. Note that we made
`v` a mutable variable here. If we didn't, the borrow checker would not allow
us to make calls to `push()`.

### Writing Elements at a Particular Index
We can also write to elements at a particular index in a similar way to how
we read elements at a particular index. We can use the indexing syntax or the
`get_mut()` method:

```rust
fn main() {
    let mut v = vec![1, 2, 3];

    let second: &mut i32 = &mut v[1];
    *second = 3;

    match v.get_mut(2) {
        Some(third) => *third = 9,
        None => println!("There is no third element."),
    }
}
```

Here, we use the indexing syntax to get a mutable reference to the second
element and change its value to `3`. We then use the `get_mut()` method to get
a mutable reference to the third element and change its value to `9`.

As with the example for reading elements at a particular index, an out-of-bounds
access with the indexing sytanx can cause a `panic` while an out-of-bounds
access with the `get_mut()` method returns `None`.

### Iterating Over Elements
We can iterate over elements in a vector with a `for` loop to mutate the values:

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    for i in &mut v {
        *i = *i + 1
    }
}
```

Here, we add `1` to each of the values in the vector. Note that the `for` loop
is mutably borrowing `v`, as shown by the `&mut v`.