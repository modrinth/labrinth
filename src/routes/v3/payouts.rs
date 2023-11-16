use actix_web::web;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("payouts")
            .route("{user_id}/projects", web::get().to(projects_list))
    );
}


// todo: webhooks (tremendous, paypal)
// todo: historical payouts get route (and link to old users route)
// todo: payment cancelling
// todo: payment withdraw (paypal, tremendous)
