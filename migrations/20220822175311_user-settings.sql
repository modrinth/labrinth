CREATE TABLE user_settings (
    user_id bigint NOT NULL PRIMARY KEY UNIQUE,
    tos_agreed boolean NOT NULL DEFAULT FALSE,
    public_email boolean NOT NULL DEFAULT FALSE,
    public_github boolean NOT NULL DEFAULT TRUE
);

INSERT INTO user_settings (user_id)
    SELECT id FROM users;
