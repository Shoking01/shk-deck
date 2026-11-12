//! The geometry's edges.
//!
//! `Geometry::new` derives the row count from the key count and the column
//! count, and `position` divides by the column count. Those two facts carry
//! the whole invariant, so they are worth pinning at the edges: zero keys,
//! zero columns, and a key count that does not divide evenly.

use deck_api::{Geometry, KeyId};

#[test]
fn a_deck_with_no_keys_has_no_rows_and_no_positions() {
    let geometry = Geometry::new(0, 3);

    assert_eq!(geometry.key_count(), 0);
    assert_eq!(geometry.rows(), 0);
    assert_eq!(geometry.columns(), 3);
    assert!(!geometry.contains(KeyId::new(0)));
    assert_eq!(geometry.position(KeyId::new(0)), None);
}

#[test]
fn a_deck_with_no_columns_is_treated_as_one_column_wide() {
    let geometry = Geometry::new(3, 0);

    assert_eq!(geometry.columns(), 1);
    assert_eq!(geometry.rows(), 3);
    assert_eq!(geometry.position(KeyId::new(0)), Some((0, 0)));
    assert_eq!(geometry.position(KeyId::new(2)), Some((2, 0)));
}

#[test]
fn a_key_count_that_does_not_divide_evenly_rounds_the_rows_up() {
    let geometry = Geometry::new(6, 4);

    assert_eq!(geometry.rows(), 2);
    assert_eq!(geometry.position(KeyId::new(3)), Some((0, 3)));
    assert_eq!(geometry.position(KeyId::new(4)), Some((1, 0)));
    assert_eq!(geometry.position(KeyId::new(5)), Some((1, 1)));
}

#[test]
fn every_position_lands_inside_the_grid_the_geometry_declares() {
    let geometry = Geometry::new(6, 4);

    for index in 0..geometry.key_count() {
        let (row, column) = geometry
            .position(KeyId::new(index))
            .expect("a key the deck has must have a position");
        assert!(
            row < geometry.rows(),
            "key {index} landed on row {row}, which the geometry does not declare"
        );
        assert!(
            column < geometry.columns(),
            "key {index} landed on column {column}, which the geometry does not declare"
        );
    }
}

#[test]
fn a_key_past_the_end_has_no_position() {
    let geometry = Geometry::new(6, 3);

    assert_eq!(geometry.position(KeyId::new(6)), None);
    assert_eq!(geometry.position(KeyId::new(1_000)), None);
}
