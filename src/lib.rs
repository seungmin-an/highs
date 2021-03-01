#![forbid(missing_docs)]
//! Safe rust binding to the [HiGHS](https://highs.dev) linear programming solver.
//!
//! ## Usage example
//!
//! ### Building a problem constraint by constraint
//!
//! Useful for traditional problem modelling where you first declare your variables, then add
//!constraints one by one.
//!
//! ```
//! use highs::{Sense, Model, HighsModelStatus, RowProblem};
//! // max: x + 2y + z
//! // under constraints:
//! // c1: 3x +  y      <= 6
//! // c2:       y + 2z <= 7
//! let mut pb = RowProblem::default();
//! let x = pb.add_column(1., 0..);
//! let y = pb.add_column(2., 0..);
//! let z = pb.add_column(1., 0..);
//! // c1
//! pb.add_row(..=6, &[(x, 3.), (y, 1.)]);
//! // c2
//! pb.add_row(..=7, &[(y, 1.), (z, 2.)]);
//!
//! let mut model = Model::default();
//! model.set_problem(pb);
//! model.set_sense(Sense::Maximise);
//!
//! let solved = model.solve();
//!
//! assert_eq!(solved.status(), HighsModelStatus::Optimal);
//!
//! let solution = solved.get_solution();
//! // The expected solution is x=0  y=6  z=0.5
//! assert_eq!(solution.columns(), vec![0., 6., 0.5]);
//! // All the constraints are at their maximum
//! assert_eq!(solution.rows(), vec![6., 7.]);
//! ```
//!
//! ### Building a problem variable by variable
//!
//! Useful for resource allocation problems and other problems when you know in advance the number
//! of constraints and their bounds, but dynamically add new variables to the problem.
//! ```
//! use highs::{Sense, Model, HighsModelStatus, ColProblem};
//! // max: x + 2y + z
//! // under constraints:
//! // c1: 3x +  y      <= 6
//! // c2:       y + 2z <= 7
//! let mut pb = ColProblem::default();
//! let c1 = pb.add_row(..6.);
//! let c2 = pb.add_row(..7.);
//! // x
//! pb.add_column(1., 0.., &[(c1, 3.)]);
//! // y
//! pb.add_column(2., 0.., &[(c1, 1.), (c2, 1.)]);
//! // z
//! pb.add_column(1., 0.., vec![(c2, 2.)]);
//!
//! let mut model = Model::default();
//! model.set_problem(pb);
//! model.set_sense(Sense::Maximise);
//!
//! let solved = model.solve();
//!
//! assert_eq!(solved.status(), HighsModelStatus::Optimal);
//!
//! let solution = solved.get_solution();
//! // The expected solution is x=0  y=6  z=0.5
//! assert_eq!(solution.columns(), vec![0., 6., 0.5]);
//! // All the constraints are at their maximum
//! assert_eq!(solution.rows(), vec![6., 7.]);
//! ```
use std::convert::{TryFrom, TryInto};
use std::ffi::c_void;
use std::ops::{Bound, RangeBounds};
use std::os::raw::c_int;

use highs_sys::*;

pub use matrix_col::{ColMatrix, Row};
pub use matrix_row::{Col, RowMatrix};
pub use status::{HighsModelStatus, HighsStatus};

/// A problem where variables are declared first, and constraints are then added dynamically.
pub type RowProblem = Problem<RowMatrix>;
/// A problem where constraints are declared first, and variables are then added dynamically.
pub type ColProblem = Problem<ColMatrix>;

mod status;
mod matrix_row;
mod matrix_col;

/// A complete optimization problem
/// Depending on the MATRIX type parameter, the problem will be built
/// constraint by constraint (with MATRIX=RowMatrix), or
/// variable by variable (with MATRIX=ColMatrix)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Problem<MATRIX = ColMatrix> {
    // columns
    colcost: Vec<f64>,
    collower: Vec<f64>,
    colupper: Vec<f64>,
    // rows
    rowlower: Vec<f64>,
    rowupper: Vec<f64>,
    matrix: MATRIX,
}

impl<MATRIX> Problem<MATRIX> {
    fn num_cols(&self) -> usize {
        self.colcost.len()
    }
    fn num_rows(&self) -> usize {
        self.rowlower.len()
    }

    fn add_row_inner<N: Into<f64> + Copy, B: RangeBounds<N>>(&mut self, bounds: B) -> Row {
        let r = Row(self.num_rows().try_into().unwrap());
        let low = bound_value(bounds.start_bound()).unwrap_or(f64::NEG_INFINITY);
        let high = bound_value(bounds.end_bound()).unwrap_or(f64::INFINITY);
        self.rowlower.push(low);
        self.rowupper.push(high);
        r
    }

    fn add_column_inner<
        N: Into<f64> + Copy,
        B: RangeBounds<N>,
    >(
        &mut self,
        col_factor: f64,
        bounds: B,
    ) {
        self.colcost.push(col_factor);
        let low = bound_value(bounds.start_bound()).unwrap_or(f64::NEG_INFINITY);
        let high = bound_value(bounds.end_bound()).unwrap_or(f64::INFINITY);
        self.collower.push(low);
        self.colupper.push(high);
    }
}

fn bound_value<N: Into<f64> + Copy>(b: Bound<&N>) -> Option<f64> {
    match b {
        Bound::Included(v) => Some((*v).into()),
        Bound::Excluded(v) => Some((*v).into()),
        Bound::Unbounded => None,
    }
}

fn c(n: usize) -> c_int {
    n.try_into().unwrap()
}


/// A model to solve
#[derive(Debug, Default)]
pub struct Model {
    highs: HighsPtr,
}

/// A solved model
#[derive(Debug)]
pub struct SolvedModel {
    highs: HighsPtr,
}

/// Whether to maximize or minimize the objective function
#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Sense {
    /// max
    Maximise = -1,
    /// min
    Minimise = 1,
}

impl Model {
    /// Create a Highs model to be optimized (but don't solve it yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the optimization sense (minimize by default)
    pub fn set_sense(&mut self, sense: Sense) {
        unsafe {
            Highs_changeObjectiveSense(self.highs.mut_ptr(), sense as c_int);
        }
    }

    /// Set the problem to optimize
    pub fn set_problem<P: Into<Problem<ColMatrix>>>(&mut self, problem: P) {
        let problem = problem.into();
        log::debug!(
            "Adding a problem with {} variables and {} constraints to HiGHS",
            problem.num_cols(),
            problem.num_rows()
        );
        unsafe {
            handle_status(Highs_passLp(
                self.highs.mut_ptr(),
                c(problem.num_cols()),
                c(problem.num_rows()),
                c(problem.matrix.avalue.len()),
                problem.colcost.as_ptr(),
                problem.collower.as_ptr(),
                problem.colupper.as_ptr(),
                problem.rowlower.as_ptr(),
                problem.rowupper.as_ptr(),
                problem.matrix.astart.as_ptr(),
                problem.matrix.aindex.as_ptr(),
                problem.matrix.avalue.as_ptr(),
            ));
        }
    }

    /// Prevents writing anything to the standard output when solving the model
    pub fn make_quiet(&mut self) {
        handle_status(unsafe { Highs_runQuiet(self.highs.mut_ptr()) })
    }

    /// Find the optimal value for the problem
    pub fn solve(mut self) -> SolvedModel {
        unsafe {
            handle_status(Highs_run(self.highs.mut_ptr()));
        }
        SolvedModel { highs: self.highs }
    }
}

impl From<SolvedModel> for Model {
    fn from(solved: SolvedModel) -> Self {
        Self {
            highs: solved.highs,
        }
    }
}

#[derive(Debug)]
struct HighsPtr(*mut c_void);

impl Drop for HighsPtr {
    fn drop(&mut self) {
        unsafe { Highs_destroy(self.0) }
    }
}

impl Default for HighsPtr {
    fn default() -> Self {
        HighsPtr(unsafe { Highs_create() })
    }
}

impl HighsPtr {
    // To be used instead of unsafe_mut_ptr wherever possible
    #[allow(dead_code)]
    fn ptr(&self) -> *const c_void {
        self.0
    }

    // Needed until https://github.com/ERGO-Code/HiGHS/issues/479 is fixed
    unsafe fn unsafe_mut_ptr(&self) -> *mut c_void {
        self.0
    }

    fn mut_ptr(&mut self) -> *mut c_void {
        self.0
    }
}

impl SolvedModel {
    /// The status of the solution. Should be Optimal if everything went well
    pub fn status(&self) -> HighsModelStatus {
        let model_status = unsafe { Highs_getModelStatus(self.highs.unsafe_mut_ptr(), 0) };
        HighsModelStatus::try_from(model_status).unwrap()
    }

    /// Get the solution to the problem
    pub fn get_solution(&self) -> Solution {
        let cols = self.num_cols();
        let rows = self.num_rows();
        let mut colvalue: Vec<f64> = vec![0.; cols];
        let mut coldual: Vec<f64> = vec![0.; cols];
        let mut rowvalue: Vec<f64> = vec![0.; rows];
        let mut rowdual: Vec<f64> = vec![0.; rows];

        // Get the primal and dual solution
        unsafe {
            Highs_getSolution(
                self.highs.unsafe_mut_ptr(),
                colvalue.as_mut_ptr(),
                coldual.as_mut_ptr(),
                rowvalue.as_mut_ptr(),
                rowdual.as_mut_ptr(),
            );
        }

        Solution {
            colvalue,
            coldual,
            rowvalue,
            rowdual,
        }
    }

    /// Number of variables
    fn num_cols(&self) -> usize {
        let n = unsafe { Highs_getNumCols(self.highs.unsafe_mut_ptr()) };
        n.try_into().unwrap()
    }

    /// Number of constraints
    fn num_rows(&self) -> usize {
        let n = unsafe { Highs_getNumRows(self.highs.unsafe_mut_ptr()) };
        n.try_into().unwrap()
    }
}

/// Concrete values of the solution
#[derive(Clone, Debug)]
pub struct Solution {
    colvalue: Vec<f64>,
    coldual: Vec<f64>,
    rowvalue: Vec<f64>,
    rowdual: Vec<f64>,
}

impl Solution {
    /// The optimal values for each variables (in the order they were added)
    pub fn columns(&self) -> &[f64] {
        &self.colvalue
    }
    /// The optimal values for each variables in the dual problem (in the order they were added)
    pub fn dual_columns(&self) -> &[f64] {
        &self.coldual
    }
    /// The value of the constraint functions
    pub fn rows(&self) -> &[f64] {
        &self.rowvalue
    }
    /// The value of the constraint functions in the dual problem
    pub fn dual_rows(&self) -> &[f64] {
        &self.rowdual
    }
}

fn handle_status(status: c_int) {
    match HighsStatus::try_from(status).unwrap() {
        HighsStatus::OK => {}
        HighsStatus::Warning => {
            log::warn!("Warning from HiGHS !");
        }
        HighsStatus::Error => {
            panic!(
                "An error was encountered in HiGHS. This is probably a memory allocation error."
            );
        }
    }
}