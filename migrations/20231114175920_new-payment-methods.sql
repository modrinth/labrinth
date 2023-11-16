ALTER TABLE users
    ADD COLUMN paypal_country text NULL,
    ADD COLUMN paypal_email text NULL,
    ADD COLUMN paypal_id text NULL,
    ADD COLUMN venmo_handle text NULL,

    DROP COLUMN midas_expires,
    DROP COLUMN is_overdue,
    DROP COLUMN stripe_customer_id,
    DROP COLUMN payout_wallet,
    DROP COLUMN payout_wallet_type,
    DROP COLUMN payout_address;


-- TODO FIGURE OUT
ALTER TABLE historical_payouts
    ADD COLUMN payment_method text NULL,
    ADD COLUMN payment_id text NULL;

UPDATE historical_payouts
SET status = 'processed'