#!/bin/bash
# setup.sh - Vision Platform Setup Script

set -e

echo "Vision Platform Setup"
echo "===================="

# Detect OS
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    OS="linux"
    DISTRO=$(lsb_release -si 2>/dev/null || echo "Unknown")
elif [[ "$OSTYPE" == "darwin"* ]]; then
    OS="macos"
else
    echo "Unsupported OS: $OSTYPE"
    exit 1
fi

echo "Detected OS: $OS ($DISTRO)"

# Install dependencies
echo "Installing system dependencies..."

if [ "$OS" = "linux" ]; then
    if [ "$DISTRO" = "Ubuntu" ] || [ "$DISTRO" = "Debian" ]; then
        sudo apt-get update
        sudo apt-get install -y \
            libgstreamer1.0-dev \
            libgstreamer-plugins-base1.0-dev \
            gstreamer1.0-plugins-base \
            gstreamer1.0-plugins-good \
            gstreamer1.0-plugins-bad \
            gstreamer1.0-plugins-ugly \
            gstreamer1.0-libav \
            gstreamer1.0-tools \
            libgstreamer-plugins-bad1.0-dev \
            libgtk-4-dev \
            libopencv-dev \
            pkg-config \
            cmake \
            build-essential \
            v4l-utils
            
        # Install VA-API for hardware acceleration (Intel/AMD)
        sudo apt-get install -y \
            gstreamer1.0-vaapi \
            vainfo
            
    elif [ "$DISTRO" = "Fedora" ] || [ "$DISTRO" = "RedHat" ]; then
        sudo dnf install -y \
            gstreamer1-devel \
            gstreamer1-plugins-base-devel \
            gstreamer1-plugins-good \
            gstreamer1-plugins-bad-free \
            gstreamer1-plugins-ugly \
            gtk4-devel \
            opencv-devel \
            pkg-config \
            cmake \
            gcc-c++ \
            v4l-utils
    else
        echo "Please install GStreamer, GTK4, and OpenCV manually for your distribution"
    fi
    
elif [ "$OS" = "macos" ]; then
    # Check if Homebrew is installed
    if ! command -v brew &> /dev/null; then
        echo "Homebrew not found. Please install it first:"
        echo "  /bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
        exit 1
    fi
    
    brew install \
        gstreamer \
        gst-plugins-base \
        gst-plugins-good \
        gst-plugins-bad \
        gst-plugins-ugly \
        gst-libav \
        gtk4 \
        opencv \
        pkg-config \
        cmake
fi

# Check camera devices
echo ""
echo "Checking camera devices..."
if [ "$OS" = "linux" ]; then
    if command -v v4l2-ctl &> /dev/null; then
        echo "Available cameras:"
        v4l2-ctl --list-devices
        
        # List formats for first camera
        if [ -e /dev/video0 ]; then
            echo ""
            echo "Formats for /dev/video0:"
            v4l2-ctl --list-formats-ext -d /dev/video0 | head -20
        fi
    fi
elif [ "$OS" = "macos" ]; then
    system_profiler SPCameraDataType
fi

# Test GStreamer installation
echo ""
echo "Testing GStreamer installation..."
if command -v gst-inspect-1.0 &> /dev/null; then
    echo "✓ GStreamer installed (version: $(gst-inspect-1.0 --version | head -1))"
    
    # Check for important plugins
    for plugin in v4l2src videotestsrc videoconvert autovideosink; do
        if gst-inspect-1.0 $plugin &> /dev/null; then
            echo "✓ Plugin '$plugin' available"
        else
            echo "✗ Plugin '$plugin' NOT available"
        fi
    done
else
    echo "✗ GStreamer not found in PATH"
    exit 1
fi

# Create default config if not exists
if [ ! -f config.toml ]; then
    echo ""
    echo "Creating default config.toml..."
    cat > config.toml << 'EOF'
[camera]
device = "/dev/video0"
width = 1280
height = 720
framerate = 30
format = "Mjpeg"

[display]
width = 1280
height = 720
fullscreen = false
show_overlay = true
show_fps = true

[processing]
enable_object_tracking = false
enable_motion_detection = true
enable_face_detection = false
processing_threads = 4

[telemetry]
enable = true
prometheus_port = 9090
log_interval_ms = 1000
EOF
    echo "✓ Created config.toml"
fi

# Download face detection cascade if needed
CASCADE_PATH="/usr/share/opencv4/haarcascades/haarcascade_frontalface_default.xml"
if [ ! -f "$CASCADE_PATH" ]; then
    echo ""
    echo "Downloading face detection model..."
    sudo mkdir -p /usr/share/opencv4/haarcascades/
    sudo wget -q https://raw.githubusercontent.com/opencv/opencv/master/data/haarcascades/haarcascade_frontalface_default.xml \
        -O "$CASCADE_PATH"
    echo "✓ Downloaded face detection model"
fi

echo ""
echo "Setup complete!"
echo ""
echo "Quick test commands:"
echo "  # Test camera with GStreamer directly:"
echo "  gst-launch-1.0 v4l2src device=/dev/video0 ! videoconvert ! autovideosink"
echo ""
echo "  # Build and run the vision platform:"
echo "  cargo build --release"
echo "  cargo run --release -- --motion --faces"
echo ""
echo "  # Run with custom settings:"
echo "  cargo run --release -- --device /dev/video0 --width 1920 --height 1080 --track"
echo ""
echo "Metrics will be available at: http://localhost:9090/metrics"