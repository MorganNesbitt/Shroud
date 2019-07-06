use amethyst::{
    assets::{AssetStorage, Handle, Loader, Processor, ProgressCounter},
    core::transform::{Transform, TransformBundle},
    ecs::prelude::{ReadExpect, Resources, SystemData},
    prelude::*,
    ui::{DrawUiDesc, UiBundle},
    input::{InputBundle, StringBindings},
    renderer::{
        sprite_visibility::SpriteVisibilitySortingSystem,
        pass::{DrawShadedDesc, DrawFlat2DDesc, DrawFlat2DTransparentDesc},
        camera::{Camera, Projection},
        rendy::{
            factory::Factory,
            graph::{
                render::{RenderGroupDesc, SubpassBuilder},
                GraphBuilder,
            },
            hal::{format::Format, image},
        },
        types::DefaultBackend,
        GraphCreator, ImageFormat, RenderingSystem, SpriteRender, SpriteSheet, SpriteSheetFormat,
        Texture, Transparent,
    },
    utils::application_root_dir,
    window::{ScreenDimensions, Window, WindowBundle},
};

struct GameState {}

impl SimpleState for GameState {
    fn on_start(&mut self, data: StateData<'_, GameData<'_, '_>>) {
        let sprite_sheet_handle = {
            let app_root = application_root_dir().expect("Could not load app root directory");
            let desert_images_path = app_root.join("resources/textures/sample/");
            let packed_image_path = desert_images_path.join("packed.png");
            let packed_image_info_path = desert_images_path.join("packed.ron");

            assert_eq!(
                packed_image_path.exists(),
                true,
                "Desert packed image path must exist"
            );
            assert_eq!(
                packed_image_info_path.exists(),
                true,
                "Desert packed image tile info path must exist"
            );

            let desert_image_path = packed_image_path
                .to_str()
                .expect("Expected image path string");
            let desert_ron_path = packed_image_info_path
                .to_str()
                .expect("Expected ron path string");

            let loader = data.world.read_resource::<Loader>();
            let texture_handle = loader.load(
                desert_image_path.to_string(),
                ImageFormat::default(),
                (),
                &data.world.read_resource::<AssetStorage<Texture>>(),
            );

            let sprite_sheet_handle = loader.load(
                desert_ron_path.to_string(),
                SpriteSheetFormat(texture_handle),
                (),
                &data.world.read_resource::<AssetStorage<SpriteSheet>>(),
            );
            sprite_sheet_handle
        };
        self.initialize_game_textures(data.world, sprite_sheet_handle);
        self.initialise_camera(data.world);
    }
}

impl GameState {
    fn initialize_game_textures(
        &mut self,
        world: &mut World,
        sprite_sheet_handle: Handle<SpriteSheet>
    ) {
        let (width, height) = {
            let dimensions = world.read_resource::<ScreenDimensions>();
            (dimensions.width(), dimensions.height())
        };

        let mut sprite_transform = Transform::default();
        sprite_transform.set_translation_xyz(width / 2., height / 2., 0.);

        let sprite_render = SpriteRender {
            sprite_sheet: sprite_sheet_handle,
            sprite_number: 0,
        };

        world
            .create_entity()
            .with(sprite_render)
            .with(sprite_transform)
            .with(Transparent)
            .build();
    }

    /// This method initialises a camera which will view our sprite.
    fn initialise_camera(&mut self, world: &mut World) {
        let (width, height) = {
            let dim = world.read_resource::<ScreenDimensions>();
            (dim.width(), dim.height())
        };

        let mut camera_transform = Transform::default();
        camera_transform.set_translation_xyz(0.0, height, 1.);

        world
            .create_entity()
            .with(camera_transform)
            // Define the view that the camera can see. It makes sense to keep the `near` value as
            // 0.0, as this means it starts seeing anything that is 0 units in front of it. The
            // `far` value is the distance the camera can see facing the origin.
            .with(Camera::from(Projection::orthographic(
                0.,
                width,
                0.,
                height,
                0.0,
                20.0,
            )))
            .build();

    }
}

fn main() -> amethyst::Result<()> {
    amethyst::start_logger(Default::default());

    let app_root = application_root_dir()?;

    let resources_dir = app_root.join("resources/");
    let display_config_path = resources_dir.join("display_config.ron");

    let game_data = GameDataBuilder::default()
        .with_bundle(WindowBundle::from_config_path(display_config_path))?
        .with_bundle(TransformBundle::new())?
        .with(
            Processor::<SpriteSheet>::new(),
            "sprite_sheet_processor",
            &[],
        )
        .with(
            SpriteVisibilitySortingSystem::new(),
            "sprite_visibility_system",
            &[],
        )
        .with_bundle(UiBundle::<DefaultBackend, StringBindings>::new())?
        .with_thread_local(RenderingSystem::<DefaultBackend, _>::new(
            RenderingGraph::default(),
        ));

    let mut game = Application::new(
        resources_dir,
        GameState {},
        game_data,
    )?;
    game.run();

    Ok(())
}

#[derive(Default)]
struct RenderingGraph {
    dimensions: Option<ScreenDimensions>,
    surface_format: Option<Format>,
    dirty: bool,
}

impl GraphCreator<DefaultBackend> for RenderingGraph {
    fn rebuild(&mut self, res: &Resources) -> bool {
        // Rebuild when dimensions change, but wait until at least two frames have the same.
        let new_dimensions = res.try_fetch::<ScreenDimensions>();
        use std::ops::Deref;
        if self.dimensions.as_ref() != new_dimensions.as_ref().map(|d| d.deref()) {
            self.dirty = true;
            self.dimensions = new_dimensions.map(|d| d.clone());
            return false;
        }

        self.dirty
    }

    fn builder(
        &mut self,
        factory: &mut Factory<DefaultBackend>,
        res: &Resources,
    ) -> GraphBuilder<DefaultBackend, Resources> {
        use amethyst::renderer::rendy::{
            graph::present::PresentNode,
            hal::command::{ClearDepthStencil, ClearValue},
        };

        self.dirty = false;
        let window = <ReadExpect<'_, Window>>::fetch(res);
        let surface = factory.create_surface(&window);
        // cache surface format to speed things up
        let surface_format = *self
            .surface_format
            .get_or_insert_with(|| factory.get_surface_format(&surface));
        let dimensions = self.dimensions.as_ref().unwrap();
        let window_kind = image::Kind::D2(dimensions.width() as u32, dimensions.height() as u32, 1, 1);

        let mut graph_builder = GraphBuilder::new();
        let color = graph_builder.create_image(
            window_kind,
            1,
            surface_format,
            Some(ClearValue::Color([0.34, 0.36, 0.52, 1.0].into())),
        );

        let depth = graph_builder.create_image(
            window_kind,
            1,
            Format::D32Sfloat,
            Some(ClearValue::DepthStencil(ClearDepthStencil(1.0, 0))),
        );

        let pass = graph_builder.add_node(
            SubpassBuilder::new()
                .with_group(DrawFlat2DDesc::new().builder()) // Draws sprites
                .with_group(DrawFlat2DTransparentDesc::new().builder()) // Draws UI components
                .with_group(DrawUiDesc::new().builder()) // Draws UI components
                .with_color(color)
                .with_depth_stencil(depth)
                .into_pass(),
        );

        let _present = graph_builder
            .add_node(PresentNode::builder(factory, surface, color).with_dependency(pass));

        graph_builder
    }
}
