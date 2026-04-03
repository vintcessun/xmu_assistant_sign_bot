use crate::api::storage::HotTable;
use std::sync::Arc;

#[tokio::test]
#[ignore]
pub async fn write_test_data() {
    let login_data: HotTable<i64, String> = HotTable::new("test_hot_table_data");
    login_data
        .insert(114514, Arc::new("1919810".to_string()))
        .unwrap();
    println!("write test data success");
    std::process::exit(0);
}

#[tokio::test]
#[ignore]
pub async fn test_login_data() {
    let login_data: HotTable<i64, String> = HotTable::new("test_hot_table_data");
    let data = login_data.get(&114514).unwrap();
    assert_eq!(data.as_str(), "1919810");
    println!("test login data: {}", data);
    std::process::exit(0);
}
