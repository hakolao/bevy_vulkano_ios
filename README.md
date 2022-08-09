# Bevy Vulkano on iOS

This app is a working example running [Bevy](https://github.com/bevyengine) and [Vulkano](https://github.com/vulkano-rs/vulkano)
on iOS.

The app bundle has been generated using [cargo-mobile](https://github.com/BrainiumLLC/cargo-mobile). `MoltenVK` framework from VulkanSDK has been added using xcode.

## Run

1. Install [cargo-mobile](https://github.com/BrainiumLLC/cargo-mobile) and make sure you have xcode installed
2. Run `cargo mobile init`
3. Open project in xcode `open gen/apple/bevy-vulkano-ios.xcodeproj`
4. Make sure your development team is selected in _Signing & Capabilities_. You will need an apple development account.
5. For this project, link `MoltenVK.xcframework`. This is necessary for `Vulkano` to work. Your SDK install might be found somewhere like `~/VulkanSDK/1.3.216.0/MoltenVK/MoltenVK.xcframework`, You can do this in XCode -> General -> Frameworks, Libraries, and Embedded Content
6. Attach your mobile device via cord to your mac
7. Run `cargo apple run`. Keep your mobile device unlocked.

## Note!
- If you get an error `error: the use of xcframeworks is not supported in the legacy build system.`, modify your xcode project
build settings in _File -> Project Settings -> Build System_: Select _New Build System_.
- Also, debugging won't work unless your xcode supports the same iOS version which your device is running.
- Winit does not have the screen flip events, thus it might be better to disable them in xcode for the project in _General -> Deployment Info -> Device Orientation_

Result: 

![game_of_life](./game_of_life.jpg)
