use crate::package::Package;

pub trait Collector {
    fn enabled(&self) -> bool;
    fn collect(&self) -> Vec<Package>;
}

pub mod pacman;
pub mod cargo;
pub mod npm;
pub mod pip;
