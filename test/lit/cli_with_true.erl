%% RUN: @firefly compile -o @tempfile @tests/cli.erl && @tempfile true

%% CHECK: <<"Hello, world!">>
