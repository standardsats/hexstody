mod runner;

use hexstody_api::types::*;
use runner::run_test;

#[tokio::test]
async fn test_simple() {
    run_test(|env| async move {
        env.hot_client.ping().await.expect("Ping finished");
    })
    .await;
}

#[tokio::test]
async fn test_signup_email() {
    run_test(|env| async move {
        let user = "aboba@mail.com".to_owned();
        let password = "123456".to_owned();

        let res = env
            .hot_client
            .signin_email(SigninEmail {
                user: user.clone(),
                password: password.clone(),
            })
            .await;
        assert!(!res.is_ok());

        env.hot_client
            .signup_email(SignupEmail {
                user: user.clone(),
                password: password.clone(),
            })
            .await
            .expect("Signup");

        env.hot_client
            .signin_email(SigninEmail {
                user: user.clone(),
                password: password.clone(),
            })
            .await
            .expect("Signin");

        let res = env
            .hot_client
            .signin_email(SigninEmail {
                user: user.clone(),
                password: "wrong".to_owned(),
            })
            .await;
        assert!(!res.is_ok(), "Wrong password passes");
    })
    .await;
}
