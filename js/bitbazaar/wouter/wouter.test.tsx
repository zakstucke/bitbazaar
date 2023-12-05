import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "bun:test";
import { Router } from "wouter";
import { memoryLocation } from "wouter/memory-location";

import { route, routeParams, RouteParamsOutput_T, routes } from ".";

const Error404 = () => <p>404</p>;

describe("Wouter", () => {
    afterEach(() => {
        cleanup();
    });

    it("basic", () => {
        const homeParams = routeParams([
            ["id", "int"],
            ["name", "string"],
        ]);
        const aboutParams = routeParams([]);

        const Home = ({ id, name }: RouteParamsOutput_T<typeof homeParams>) => {
            expect(id).toBe(5);
            expect(name).toBe("foo");
            return <p>Home</p>;
        };

        const About = ({}: RouteParamsOutput_T<typeof aboutParams>) => <p>About</p>;

        const myRoutes = routes({
            Comp404: Error404,
            routes: {
                home: route({
                    base: "/home",
                    Comp: Home,
                    params: homeParams,
                }),
                about: route({
                    base: "/about",
                    Comp: About,
                    params: aboutParams,
                }),
            },
        });

        const { hook, navigate } = memoryLocation();
        const comp = <Router hook={hook}>{myRoutes.switch}</Router>;

        const { rerender } = render(comp);

        // Should match home:
        navigate(
            myRoutes.paths.home.build({
                id: 5,
                name: "foo",
            }),
        );
        rerender(comp);
        screen.getByText("Home");

        // Navigate to about and should match:
        navigate(myRoutes.paths.about.build({}));
        rerender(comp);
        screen.getByText("About");

        // Shouldn't match something completely random:
        navigate("/sdfsdfsd");
        rerender(comp);
        screen.getByText("404");

        // Shouldn't match when the coecion fails:
        navigate("/home/notAnInt/foo");
        rerender(comp);
        screen.getByText("404");
    });

    it.each([
        ["int", "5", 5],
        ["int", "5.5", 5],
        ["int", "sdfsdf", null],
        ["float", "5", 5],
        ["float", "5.5", 5.5],
        ["float", "sdfsdf", null],
        ["string", "5", "5"],
        ["string", "sdfsdf", "sdfsdf"],
        ["boolean", "true", true],
        ["boolean", "True", true],
        ["boolean", "1", true],
        ["boolean", "y", true],
        ["boolean", "Y", true],
        ["boolean", "false", false],
        ["boolean", "False", false],
        ["boolean", "0", false],
        ["boolean", "n", false],
        ["boolean", "N", false],
        ["boolean", "sdkfjsd", null],
    ])(
        "Coercion: %i param_type with input %i should output %i (null if 404)",
        (param_type, input, outputOrNull) => {
            const homeParams = routeParams([["id", param_type as any]]);

            const Home = ({ id }: RouteParamsOutput_T<typeof homeParams>) => {
                if (outputOrNull !== null) {
                    expect(id).toBe(outputOrNull);
                }
                return <p>Home</p>;
            };

            const myRoutes = routes({
                Comp404: Error404,
                routes: {
                    home: route({
                        base: "/",
                        Comp: Home,
                        params: [["id", param_type as any]],
                    }),
                },
            });

            const { hook, navigate } = memoryLocation();
            const comp = <Router hook={hook}>{myRoutes.switch}</Router>;

            navigate(`/${input}`);
            render(comp);

            if (outputOrNull !== null) {
                screen.getByText("Home");
            } else {
                screen.getByText("404");
            }
        },
    );
});
