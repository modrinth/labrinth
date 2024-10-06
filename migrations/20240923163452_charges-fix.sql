CREATE TABLE charges (
    id bigint PRIMARY KEY,
    user_id bigint REFERENCES users NOT NULL,
    price_id bigint REFERENCES products_prices NOT NULL,
    amount bigint NOT NULL,
    currency_code text NOT NULL,
    subscription_id bigint NULL,
    interval text NULL,
    status varchar(255) NOT NULL,
    due timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_attempt timestamptz NOT NULL
);

ALTER TABLE users_subscriptions DROP COLUMN last_charge;
ALTER TABLE users_subscriptions ADD COLUMN metadata jsonb NULL;