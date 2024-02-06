# ASE-Project Group 1 Project Proposal / Draft

## Motivation
Spatial audio enhances listeners' experiences, providing a more realistic and immersive audio environment. This technology holds the potential to benefit listeners across various industries. As more advanced spatial computing emerges, spatial audio becomes instrumental in elevating the user experience of these products even further.

## Problem to be solved
We want to develop an audio effect software incorporating sound localization and reverberation to simulate spatial characteristics. We intend to maintain the output in binaural format, which, in comparison to ambisonics, has restricted dimensions for spatial audio. Consequently, the primary issues to address include binaural panning, determining the distance and depth of audio sources, and achieving realistic room acoustics for reverberation.

## Need for this project
* 3D binaural audio production
* GUI for the users

## Applications
* Game development (sound design)
* Music Production
* AR/VR

## Functionality/Differentiation
We want to design software comparable to the state-of-the-art spatializer dearVR PRO 2 developed by Dear Reality. It can handle multiple types of input formats such as multi-channel, binaural, and Ambisonics. It also has multiple functionalities, such as stereo width control, selectable acoustic environments, reflection effects, and HP/LP filters for effects. Our goal is to include the stereo width, and acoustic environments functionality in our software. To set ourselves apart from dearVR PRO, we plan to allow users to input their own recorded impulse response, enabling them to create personalized reverberation within acoustic environments.

## Implementation
1. Input Audio Processing
2. Multi-Channel Format Conversion
3. Early Reflection Generation
4. Spatial Rendering
5. Filter Processing
6. User Interface Interaction

## References (algorithmic)
* https://docs.rs/ambisonic/latest/ambisonic/
* https://docs.rs/rodio/latest/rodio/source/struct.Spatial.html

## Work Assignments
* Audio processing
* Spatial rendering
* Implementing the user interface
