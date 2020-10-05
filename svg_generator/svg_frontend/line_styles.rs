pub enum LineStyle {
    OwnerLine,
    RefValueLine,
    RefDataLine,
}

pub enum OwnerLine {
    // what is the owner's current status?
    Solid,     // you can assign to, write and read from this owner
    Hollow,    // you can only read the data from this RAP.
    Dotted,    // you cannot read nor write the data from this RAP temporarily (borrowed away by a mut reference)
    Empty,     // you cannot read nor write the data from this RAP forever (moved)
}


// Will show up as colorful or grayed out
pub enum RefValueLine {
    // can you reassign the ref (self) to something else right after this line? Depend on & or &mut 
    // (i.e. static object will never be re-assignable. For mutable objects, it depends)
    // if there is a current borrowing on a mutable object, then it is not reassignable
    
    // This will show up as a colorful line
    Reassignable,           // this ref is declared as 
                            // mut ref = &x;
                            // or 
                            // mut ref = &mut x;
                            // and currently no second-level reference is borrowing from it. So you can in fact reassign it. 
    // This will be a grayed out line
    NotReassignable,        // anything else.
}

// NOTE that the style coincide with OwnerLine, but we use the data structure to distinguish the semantics internally 
pub enum RefDataLine {
    // can you change the data this reference points to? I.e. you might need to dereference multiple times
    Solid,     // we can r/w data
    Hollow,    // can only read data
    Dotted,    // cannot read or write, because self is borrowed by yet another &mut
    Empty,     // you cannot read nor write the data from this RAP forever (after last use)
}
