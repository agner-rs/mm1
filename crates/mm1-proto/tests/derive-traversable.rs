use std::convert::Infallible;

use mm1_proto::{AnyError, Traversable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell(i32);

impl Traversable<Vec<i32>, Vec<i32>> for Cell {
    type AdjustError = AnyError;
    type InspectError = Infallible;

    fn inspect(&self, visitor: &mut Vec<i32>) -> Result<(), Self::InspectError> {
        visitor.push(self.0);
        Ok(())
    }

    fn adjust(&mut self, visitor: &mut Vec<i32>) -> Result<(), Self::AdjustError> {
        self.0 = visitor.pop().ok_or("underrun")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Traversable)]
pub struct UnitStruct;

#[derive(Debug, Clone, PartialEq, Eq, Traversable)]
pub struct TupleStruct(UnitStruct, Cell);

#[derive(Debug, Clone, PartialEq, Eq, Traversable)]
pub struct FieldStruct {
    left:  TupleStruct,
    right: TupleStruct,
}

#[derive(Traversable)]
pub struct GenArray<const WIDTH: usize>([u8; WIDTH]);

#[test]
fn noop() {
    let input = FieldStruct {
        left:  TupleStruct(UnitStruct, Cell(123)),
        right: TupleStruct(UnitStruct, Cell(321)),
    };
    let mut ctx = Default::default();
    Traversable::<Vec<i32>, Vec<i32>>::inspect(&input, &mut ctx).expect("inspect");
    let mut output = input.clone();
    Traversable::<Vec<i32>, Vec<i32>>::adjust(&mut output, &mut ctx).expect("adjust");

    assert_ne!(output, input);
    assert_eq!(
        output,
        FieldStruct {
            left:  TupleStruct(UnitStruct, Cell(321)),
            right: TupleStruct(UnitStruct, Cell(123)),
        }
    )
}
