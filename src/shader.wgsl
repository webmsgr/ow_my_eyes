struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) in_vertex_index: u32,
) -> VertexOutput {
    var out: VertexOutput;
    var x: f32 = 0;
    var y: f32 = 0;

    // Map the vertex_index to a full-screen triangle
    if (in_vertex_index == 0u) {
        x = -1.0;
        y = -1.0;
    } else if (in_vertex_index == 1u) {
        x = 3.0;
        y = -1.0;
    } else { // in_vertex_index == 2u
        x = -1.0;
        y = 3.0;
    }

    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}
const WGScale: u32 = 120; // 120*16 = 1920. 120*9 = 1080

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // this needs to read the right location in our buffer and display the pixel accordingly
    let x = in.clip_position.x;
    let y = in.clip_position.y;
    let index = u32(x) + u32(y) * 1920;

    let pix = output[index];
    var r = f32(0);
    var g = f32(0);
    var b = f32(0);

    if pix == 0 {
        r = 1.0;
    } else if pix == 1 {
        g = 1.0;
    } else if pix == 2 {
        b = 1.0;
    }
    return vec4<f32>(r,g,b, 1.0);
}


// no fucking idea how to write shaders lol.
@group(0) @binding(0) var<storage, read_write> output: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<u32>;
@compute @workgroup_size(16, 9, 1)
fn compute(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
    @builtin(local_invocation_id) local_invocation_id : vec3<u32>,
    @builtin(global_invocation_id) global_invocation_id : vec3<u32>,
    @builtin(local_invocation_index) local_invocation_index: u32,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    
   let global_invocation_index = global_invocation_id.y * 1920 + global_invocation_id.x;
    //output[global_invocation_index] = u32(69);
    let x = global_invocation_index % 1920;
    let y = global_invocation_index / 1920;
    // PixelState::Rock => 0,
    // PixelState::Paper => 1,
    // PixelState::Scissors => 2,
    var wins_against_us = u32(0);
    let us = input[global_invocation_index];
    if us == 0 {
        wins_against_us = u32(1);
    } else if us == 1 {
        wins_against_us = u32(2);
    } else if us == 2 {
        wins_against_us = u32(0);
    }
    //var ou = i32(10) / i32(0);
    var win_count = u32(0);
    // check all 8 neighbors (in wgsl)
    // 0 1 2
    // 3 x 4
    // 5 6 7
    // 0
    if x > 0 {
        if input[global_invocation_index - 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 1
    if x > 0 && y > 0 {
        if input[global_invocation_index - 1920 - 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 2
    if y > 0 {
        if input[global_invocation_index - 1920] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 3
    if x < 1920 - 1 && y > 0 {
        if input[global_invocation_index - 1920 + 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 4
    if x < 1920 - 1 {
        if input[global_invocation_index + 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 5
    if x < 1920 - 1 && y < 1080 - 1 {
        if input[global_invocation_index + 1920 + 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 6
    if y < 1080 - 1 {
        if input[global_invocation_index + 1920] == wins_against_us {
            win_count += u32(1);
        }
    }
    // 7
    if x > 0 && y < 1080 - 1 {
        if input[global_invocation_index + 1920 - 1] == wins_against_us {
            win_count += u32(1);
        }
    }
    
    if win_count > 2 {
        output[global_invocation_index] = wins_against_us;
    } else {
        output[global_invocation_index] = us;
    }
    //output[global_invocation_index] = u32(0);

}