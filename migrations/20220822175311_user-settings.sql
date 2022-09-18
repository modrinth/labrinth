CREATE TABLE user_settings (
    user_id bigint NOT NULL PRIMARY KEY UNIQUE,
    public_email boolean NOT NULL DEFAULT FALSE,
    public_github boolean NOT NULL DEFAULT TRUE,
    theme varchar(32) NOT NULL DEFAULT 'system',
    locale varchar(32) NOT NULL DEFAULT 'en-US'
);

INSERT INTO user_settings (user_id)
    SELECT id FROM users;
