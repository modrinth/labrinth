use chrono::{DateTime, Duration, Utc};
use common::database::*;
use itertools::Itertools;
use labrinth::models::ids::base62_impl::parse_base62;
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::common::environment::TestEnvironment;

// importing common module.
mod common;

#[actix_rt::test]
pub async fn analytics_revenue() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let alpha_project_id = test_env
        .dummy
        .as_ref()
        .unwrap()
        .project_alpha
        .project_id
        .clone();

    let pool = test_env.db.pool.clone();

    // Generate sample revenue data- directly insert into sql
    let (mut insert_user_ids, mut insert_project_ids, mut insert_payouts, mut insert_starts) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    // Note: these go from most recent to least recent
    let money_time_pairs: [(f64, DateTime<Utc>); 10] = [
        (50.0, Utc::now() - Duration::minutes(5)),
        (50.1, Utc::now() - Duration::minutes(10)),
        (101.0, Utc::now() - Duration::days(1)),
        (200.0, Utc::now() - Duration::days(2)),
        (311.0, Utc::now() - Duration::days(3)),
        (400.0, Utc::now() - Duration::days(4)),
        (526.0, Utc::now() - Duration::days(5)),
        (633.0, Utc::now() - Duration::days(6)),
        (800.0, Utc::now() - Duration::days(14)),
        (800.0, Utc::now() - Duration::days(800)),
    ];

    let project_id = parse_base62(&alpha_project_id).unwrap() as i64;
    for (money, time) in money_time_pairs.iter() {
        insert_user_ids.push(USER_USER_ID_PARSED);
        insert_project_ids.push(project_id);
        insert_payouts.push(Decimal::from_f64_retain(*money).unwrap());
        insert_starts.push(*time);
    }

    sqlx::query!(
        "
        INSERT INTO payouts_values (user_id, mod_id, amount, created)
        SELECT * FROM UNNEST ($1::bigint[], $2::bigint[], $3::numeric[], $4::timestamptz[])
        ",
        &insert_user_ids[..],
        &insert_project_ids[..],
        &insert_payouts[..],
        &insert_starts[..]
    )
    .execute(&pool)
    .await
    .unwrap();

    let day = 86400;

    // Test analytics endpoint with default values
    // - all time points in the last 2 weeks
    // - 1 day resolution
    let analytics = api
        .get_analytics_revenue_deserialized(
            vec![&alpha_project_id],
            None,
            None,
            None,
            USER_USER_PAT,
        )
        .await;
    assert_eq!(analytics.len(), 1); // 1 project
    let project_analytics = analytics.get(&alpha_project_id).unwrap();
    assert_eq!(project_analytics.len(), 8); // 1 days cut off, and 2 points take place on the same day. note that the day exactly 14 days ago is included
                                            // sorted_by_key, values in the order of smallest to largest key
    let (sorted_keys, sorted_by_key): (Vec<i64>, Vec<Decimal>) = project_analytics
        .iter()
        .sorted_by_key(|(k, _)| *k)
        .rev()
        .unzip();
    assert_eq!(
        vec![100.1, 101.0, 200.0, 311.0, 400.0, 526.0, 633.0, 800.0],
        to_f64_vec_rounded_up(sorted_by_key)
    );
    // Ensure that the keys are in multiples of 1 day
    for k in sorted_keys {
        assert_eq!(k % day, 0);
    }

    // Test analytics with last 900 days to include all data
    // keep resolution at default
    let analytics = api
        .get_analytics_revenue_deserialized(
            vec![&alpha_project_id],
            Some(Utc::now() - Duration::days(801)),
            None,
            None,
            USER_USER_PAT,
        )
        .await;
    let project_analytics = analytics.get(&alpha_project_id).unwrap();
    assert_eq!(project_analytics.len(), 9); // and 2 points take place on the same day
    let (sorted_keys, sorted_by_key): (Vec<i64>, Vec<Decimal>) = project_analytics
        .iter()
        .sorted_by_key(|(k, _)| *k)
        .rev()
        .unzip();
    assert_eq!(
        vec![100.1, 101.0, 200.0, 311.0, 400.0, 526.0, 633.0, 800.0, 800.0],
        to_f64_vec_rounded_up(sorted_by_key)
    );
    for k in sorted_keys {
        assert_eq!(k % day, 0);
    }

    // Cleanup test db
    test_env.cleanup().await;
}

fn to_f64_rounded_up(d: Decimal) -> f64 {
    d.round_dp_with_strategy(1, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_f64()
        .unwrap()
}

fn to_f64_vec_rounded_up(d: Vec<Decimal>) -> Vec<f64> {
    d.into_iter().map(to_f64_rounded_up).collect_vec()
}
