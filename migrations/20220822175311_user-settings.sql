CREATE TABLE user_settings (
    user_id bigint NOT NULL PRIMARY KEY UNIQUE,
    public_github boolean NOT NULL DEFAULT TRUE,
    theme varchar(32) NOT NULL DEFAULT 'auto',
    locale varchar(32) NOT NULL DEFAULT 'auto'
);

INSERT INTO user_settings (user_id)
    SELECT id FROM users;