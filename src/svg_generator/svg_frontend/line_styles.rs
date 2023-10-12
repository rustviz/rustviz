pub enum OwnerLine {
    // what is the owner's current status?
    Solid,     // you can assign to, write and read from this owner
    Hollow,    // you can only read the data from this RAP.
    Dotted,    // you cannot read nor write the data from this RAP temporarily (borrowed away by a mut reference)
    Empty,     // you cannot read nor write the data from this RAP forever (moved)
}
