#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

impl defmt::Format for Direction {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Direction::In => defmt::write!(f, "In"),
            Direction::Out => defmt::write!(f, "Out"),
        }
    }
}
