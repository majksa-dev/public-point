use gateway::{Id, Request, Router, RouterBuilder};

#[derive(Debug, Default)]
pub struct AnyRouter;

impl Router for AnyRouter {
    fn matches(&self, _request: &Request) -> Option<Id> {
        Some(0)
    }
}

pub struct AnyRouterBuilder;

impl RouterBuilder for AnyRouterBuilder {
    fn build(self: Box<Self>) -> (Vec<String>, Box<dyn Router>) {
        (vec![String::new()], Box::new(AnyRouter))
    }
}
