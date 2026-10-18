#[path = "../src/sample_times.rs"]
mod sample_times;

use sample_times::{check_sample_times, SamplePlan};

#[test]
fn check_sample_times_covers_empty_one_order_negative_and_nan() {
    assert_eq!(check_sample_times(&[]).unwrap(), SamplePlan::Single);
    assert_eq!(
        check_sample_times(&[1.5]).unwrap(),
        SamplePlan::Repeated(vec![1.5])
    );
    assert_eq!(
        check_sample_times(&[2.0, 0.0, 1.25]).unwrap(),
        SamplePlan::Repeated(vec![2.0, 0.0, 1.25])
    );
    assert!(check_sample_times(&[-0.01]).is_err(), "negative");
    assert!(check_sample_times(&[1.0, -2.0]).is_err(), "negative later");
    assert!(check_sample_times(&[f64::NAN]).is_err(), "nan");
    assert!(check_sample_times(&[0.5, f64::NAN]).is_err(), "nan later");
    assert!(check_sample_times(&[f64::INFINITY]).is_err(), "non-finite");
    assert!(
        check_sample_times(&[f64::NEG_INFINITY]).is_err(),
        "non-finite"
    );
}
