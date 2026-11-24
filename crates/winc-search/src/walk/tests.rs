//! 遍历规则的测试：名字的自然排序。

use super::*;

#[test]
fn natural_order() {
    let mut names = vec!["file10", "File2", "file1", "a", "B"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, vec!["a", "B", "file1", "File2", "file10"]);

    // 只差大小写或前导零的名字也有确定的先后，相同的名字才相等。
    let mut names = vec!["file01", "a.rs", "file1", "A.rs", "file001"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, vec!["A.rs", "a.rs", "file1", "file01", "file001"]);
    assert_eq!(natural_cmp("x01y2", "x1y02"), Ordering::Greater);
    assert_eq!(natural_cmp("a.rs", "a.rs"), Ordering::Equal);
}
