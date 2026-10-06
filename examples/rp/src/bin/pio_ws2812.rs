//! This example shows powerful PIO module in the RP2040 chip to communicate with WS2812 LED modules.
//! See (https://www.sparkfun.com/categories/tags/ws2812)

#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::gpio;
use embassy_time::{Duration, Ticker};
use libm::sinf;
use panic_probe as _;
use smart_leds::RGB8;
use gpio::{Level, Output};
use core::f32::consts::PI;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    DMA_IRQ_0 => embassy_rp::dma::InterruptHandler<DMA_CH0>;
});

const GAMMA: f32 = 2.2;
const BRIGHTNESS: f32 = 1.0;


fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    // calculate r, g, b weightings from hue
    let r =       (h * 6.0 - 3.0).abs() - 1.0;
    let g = 2.0 - (h * 6.0 - 2.0).abs();
    let b = 2.0 - (h * 6.0 - 4.0).abs();
    // apply saturation and luminance to the colour mix
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let r = (r - 0.5) * c + l;
    let g = (g - 0.5) * c + l;
    let b = (b - 0.5) * c + l;
    // clamp the weightings to the normalised range (0-1)
    let r = r.clamp(0.0, 1.0);
    let g = g.clamp(0.0, 1.0);
    let b = b.clamp(0.0, 1.0);
    (r, g, b)
}

fn gamma(encoded: (f32, f32, f32)) -> RGB8 {
    let brightness: f32 = 255.0*libm::powf(BRIGHTNESS, GAMMA); // global brightness setting, range 0.0 - 255.0
    RGB8::new(libm::ceilf(libm::powf(encoded.0, GAMMA)*brightness) as u8,
        libm::ceilf(libm::powf(encoded.1, GAMMA)*brightness) as u8,
        libm::ceilf(libm::powf(encoded.2, GAMMA)*brightness) as u8)
}

fn shader_hsl5(x: i32, y: i32) -> impl Fn(f32) -> (f32, f32, f32) {
    let offset = 1.0;
    let x = x as f32;
    let _y = y as f32;
    
    move|time| {
        // hue oscillates from 0 to 1 and back over the course of 12 seconds
        let hue = ((time + x) % 60.0)/ 60.0; // 0.0 to 1.0
        let sat = 1.0; // full saturation
        let lum = offset/2.0; // adjust the luminance as needed
        hsl_to_rgb(hue, sat, lum)
    }
}

fn shader_hsl6(x: i32, y: i32) -> impl Fn(f32) -> (f32, f32, f32) {
    let x = x as f32;
    let _y = y as f32;
    
    move|time| {
        // hue oscillates from 0 to 1 and back over the course of ? seconds
        let time = time * 180.0; // speed up the hue change
        let radians = (time/180.0) * PI; // convert time to radians for sine function
        let time = sinf(radians) * 30.0 + 30.0; // apply a sine wave to the time value for a smooth oscillation
        let hue = ((time + x) % 60.0)/ 60.0; // 0.0 to 1.0
        let sat = 1.0; // full saturation
        let lum = 0.5; // adjust the luminance as needed
        hsl_to_rgb(hue, sat, lum)
    }
}

#[embassy_executor::main(executor = "embassy_rp::executor::Executor", entry = "cortex_m_rt::entry")]
async fn main(_spawner: Spawner) {
    info!("Start");
    // Initialize the RP2040 peripherals and PIO
    let p = embassy_rp::init(Default::default());
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);

    // This is the number of leds in the string.
    const NUM_LEDS: usize = 60; // 60 LEDs in the string. Change this to match your setup.
    let mut data = [RGB8::default(); NUM_LEDS]; // This is the array of RGB values to send to the WS2812.
    let mut _pwr = Output::new(p.PIN_26, Level::Low); // Power on the NeoPixel controller Pico Zero board. Not needed for other boards.

    // Common neopixel pins:
    // Thing plus: 8
    // Adafruit Feather: 16;  Adafruit Feather+RFM95: 4
    // NeoPixel controller Pico Zero board: 22 (active-low enable on 26)
    let program = PioWs2812Program::new(&mut common);
    let mut ws2812 = PioWs2812::new(&mut common, sm0, p.PIN_22, p.DMA_CH0, Irqs, &program);

    // Create an array of shader functions, one for each LED.
    // Each shader function takes a time value and returns an RGB value,
    // and is parameterised by the LED's location in the array.
    // The shader array is immutable as the shaders themselves are stateless,
    // i.e. will always return the same result for the same 'time' input,
    // and do not need to change during the animation.
    let shaders: [_; NUM_LEDS] = core::array::from_fn(|i| shader_hsl6(i as i32, 0));

    const FRAME_RATE: f32 = 100.0; // frames per second
    const FRAME_SECS: f32 = 1.0 / FRAME_RATE; // duration of each frame in seconds
    const FRAME_DURATION: Duration = Duration::from_millis((FRAME_SECS * 1000.0) as u64); // duration of each frame in milliseconds
    // Create a ticker to control the frame rate of the animation.
    let mut ticker = Ticker::every(FRAME_DURATION);
    let mut time: f32 = 0.0; // elapsed time in seconds, used to drive the shader functions

    // Loop forever making RGB values and pushing them out to the WS2812.
    loop {
//        debug!("New Colors:");
        for i in 0..NUM_LEDS {
            data[i] = gamma(shaders[i](time));
//            debug!("R: {} G: {} B: {}", data[i].r, data[i].g, data[i].b);
        }
        ws2812.write_slice(&data).await;

        ticker.next().await;
        time += FRAME_SECS; // Increment time by the frame duration for the next frame
    }
}
