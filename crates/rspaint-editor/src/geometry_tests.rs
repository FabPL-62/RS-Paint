#[cfg(test)]
mod tests {
    use crate::helpers::{snap_to_45, square_end};

    #[test]
    fn square_end_equalizes_sides() {
        assert_eq!(square_end((0, 0), (10, 3)), (10, 10));
        assert_eq!(square_end((0, 0), (3, -10)), (10, -10));
        assert_eq!(square_end((5, 5), (5, 5)), (5, 5));
    }

    #[test]
    fn snap_to_45_snaps_to_eight_directions() {
        assert_eq!(snap_to_45((0, 0), (10, 1)), (10, 0), "near horizontal");
        assert_eq!(snap_to_45((0, 0), (5, 5)), (5, 5), "exactly diagonal");
        assert_eq!(snap_to_45((0, 0), (1, 10)), (0, 10), "near vertical");
        assert_eq!(snap_to_45((0, 0), (-5, 5)), (-5, 5), "up-left diagonal");
    }
}
